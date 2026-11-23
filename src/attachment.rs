//! Keep inserted images beside the note so the Markdown works offline.
use image::ImageEncoder;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

pub enum Image {
    File(PathBuf),
    #[cfg(target_os = "macos")]
    Png(Vec<u8>),
    Pixels(arboard::ImageData<'static>),
}

pub fn supported(path: &Path) -> bool {
    matches!(
        image::ImageFormat::from_path(path),
        Ok(image::ImageFormat::Png
            | image::ImageFormat::Jpeg
            | image::ImageFormat::Gif
            | image::ImageFormat::WebP)
    )
}

pub fn clipboard() -> Result<Vec<Image>, arboard::Error> {
    let mut clipboard = arboard::Clipboard::new()?;
    if let Ok(paths) = clipboard.get().file_list()
        && !paths.is_empty()
        && paths.iter().all(|p| supported(p))
    {
        return Ok(paths.into_iter().map(Image::File).collect());
    }
    #[cfg(target_os = "macos")]
    if let Some(bytes) = crate::mac_png::read() {
        return Ok(vec![Image::Png(bytes)]);
    }
    match clipboard.get_image() {
        Ok(image) => Ok(vec![Image::Pixels(image)]),
        Err(arboard::Error::ContentNotAvailable) => Ok(Vec::new()),
        Err(e) => Err(e),
    }
}

pub fn store(note: &Path, image: Image) -> io::Result<String> {
    let parent = note
        .parent()
        .ok_or_else(|| io::Error::other("The note needs a parent folder"))?;
    let image = match image {
        Image::File(path) => Image::File(path.canonicalize()?),
        pixels => pixels,
    };
    let parent = parent.canonicalize()?;
    let parent = parent.as_path();
    let folder = parent
        .join(
            note.file_name()
                .ok_or_else(|| io::Error::other("Missing note filename"))?,
        )
        .with_extension("assets");
    let (stem, ext) = match &image {
        Image::File(path) => {
            if !fs::metadata(path)?.is_file() {
                return Err(io::Error::other("The image must be a regular file"));
            }
            if let Ok(relative) = path.strip_prefix(parent) {
                return markup(
                    path.file_stem().and_then(|s| s.to_str()).unwrap_or("Image"),
                    relative,
                );
            }
            (
                path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("Image")
                    .to_owned(),
                path.extension()
                    .and_then(|s| s.to_str())
                    .ok_or_else(|| io::Error::other("Missing image extension"))?
                    .to_owned(),
            )
        }
        #[cfg(target_os = "macos")]
        Image::Png(_) => ("Pasted image".to_owned(), "png".to_owned()),
        Image::Pixels(_) => ("Pasted image".to_owned(), "png".to_owned()),
    };
    fs::create_dir_all(&folder)?;
    let mut staged = tempfile::NamedTempFile::new_in(&folder)?;
    match image {
        Image::File(path) => {
            io::copy(&mut fs::File::open(path)?, &mut staged)?;
        }
        #[cfg(target_os = "macos")]
        Image::Png(bytes) => {
            use io::Write;
            image::ImageReader::with_format(io::Cursor::new(&bytes), image::ImageFormat::Png)
                .into_dimensions()
                .map_err(io::Error::other)?;
            staged.write_all(&bytes)?;
        }
        Image::Pixels(pixels) => {
            let w = u32::try_from(pixels.width).map_err(io::Error::other)?;
            let h = u32::try_from(pixels.height).map_err(io::Error::other)?;
            if w == 0
                || h == 0
                || pixels
                    .width
                    .checked_mul(pixels.height)
                    .and_then(|n| n.checked_mul(4))
                    != Some(pixels.bytes.len())
            {
                return Err(io::Error::other("Invalid clipboard image dimensions"));
            }
            image::codecs::png::PngEncoder::new(&mut staged)
                .write_image(&pixels.bytes, w, h, image::ExtendedColorType::Rgba8)
                .map_err(io::Error::other)?;
        }
    }
    staged.as_file().sync_all()?;
    for n in 1.. {
        let filename = if n == 1 {
            format!("{stem}.{ext}")
        } else {
            format!("{stem}-{n}.{ext}")
        };
        let path = folder.join(filename);
        let markdown = markup(&stem, path.strip_prefix(parent).map_err(io::Error::other)?)?;
        match staged.persist_noclobber(path) {
            Ok(_) => return Ok(markdown),
            Err(e) if e.error.kind() == io::ErrorKind::AlreadyExists => staged = e.file,
            Err(e) => return Err(e.error),
        }
    }
    unreachable!()
}

fn markup(label: &str, relative: &Path) -> io::Result<String> {
    const RESERVED: &percent_encoding::AsciiSet = &percent_encoding::CONTROLS
        .add(b'%')
        .add(b'#')
        .add(b'?')
        .add(b':');
    let mut dest = String::new();
    for part in relative.components() {
        if !dest.is_empty() {
            dest.push('/');
        }
        let part = part
            .as_os_str()
            .to_str()
            .ok_or_else(|| io::Error::other("The image filename is not UTF-8"))?;
        dest.push_str(&percent_encoding::utf8_percent_encode(part, RESERVED).to_string());
    }
    Ok(crate::link::Link::image_markdown(label, &dest))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    #[test]
    fn dropped_filename_with_a_colon_resolves_as_a_local_image() {
        use pulldown_cmark::{Event, Parser, Tag};
        let dir = tempfile::tempdir().unwrap();
        let parent = dir.path().canonicalize().unwrap();
        let note = parent.join("Field notes.md");
        let file = parent.join("data:field #1 (café).png");
        image::RgbaImage::new(1, 1).save(&file).unwrap();
        let md = store(&note, Image::File(file.clone())).unwrap();
        let dest = Parser::new(&md)
            .find_map(|event| match event {
                Event::Start(Tag::Image { dest_url, .. }) => Some(dest_url),
                _ => None,
            })
            .unwrap();
        let resolved = url::Url::from_file_path(note).unwrap().join(&dest).unwrap();
        assert_eq!(resolved.to_file_path().unwrap(), file);
    }

    #[test]
    fn imported_pixels_and_drops_survive_reopening_without_overwriting() {
        let dir = tempfile::tempdir().unwrap();
        let other = tempfile::tempdir().unwrap();
        let note = dir.path().join("Field notes.md");
        fs::write(&note, "").unwrap();
        let pixels = Image::Pixels(arboard::ImageData {
            width: 1,
            height: 1,
            bytes: vec![1, 2, 3, 255].into(),
        });
        let first = store(&note, pixels).unwrap();
        assert_eq!(
            first,
            "![Pasted image](Field%20notes.assets/Pasted%20image.png)"
        );
        let saved = note.with_extension("assets").join("Pasted image.png");
        assert_eq!(
            image::open(&saved).unwrap().to_rgba8().as_raw(),
            &[1, 2, 3, 255]
        );
        let external = other.path().join("Pasted image.png");
        fs::copy(&saved, &external).unwrap();
        let second = store(&note, Image::File(external)).unwrap();
        assert!(second.ends_with("Pasted%20image-2.png)"));
        assert_eq!(store(&note, Image::File(saved)).unwrap(), first);
        let mut doc = crate::document::Document::open(&note).unwrap();
        doc.text = format!("{first}\n\n{second}");
        doc.save(&note).unwrap();
        assert_eq!(
            crate::document::Document::open(&note).unwrap().text,
            doc.text
        );
    }
}
