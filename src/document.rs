use folio::{FileFormat, block_prefix, task_prefix};
use std::{
    fs,
    io::{self, Write},
    ops::Range,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct Document {
    pub text: String,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    format: FileFormat,
    last_saved: Vec<u8>,
}

impl Document {
    pub fn open(path: &Path) -> io::Result<Self> {
        let path = path.canonicalize()?;
        let bytes = fs::read(&path)?;
        let (text, format) = FileFormat::decode(&bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "This file is not UTF-8 Markdown.",
            )
        })?;
        Ok(Self {
            text,
            format,
            path: Some(path),
            last_saved: bytes,
            dirty: false,
        })
    }
    pub fn save(&mut self, destination: &Path) -> io::Result<()> {
        let path = if destination.exists() {
            destination.canonicalize()?
        } else {
            destination.to_owned()
        };
        if self.path.as_ref() == Some(&path) && fs::read(&path)? != self.last_saved {
            return Err(io::Error::other(
                "This file changed on disk. Use Save As to keep both versions.",
            ));
        }
        let bytes = self.format.encode(&self.text);
        let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
        if let Ok(meta) = fs::metadata(&path) {
            tmp.as_file().set_permissions(meta.permissions())?;
        }
        tmp.write_all(&bytes)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&path)?;
        self.path = Some(path);
        self.last_saved = bytes;
        self.dirty = false;
        Ok(())
    }
    pub fn name(&self) -> &str {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
    }
}

#[derive(Clone, Copy)]
pub enum Format {
    Block(&'static str),
    Wrap(&'static str),
    Task,
}

pub fn byte_at(text: &str, character: usize) -> usize {
    text.char_indices()
        .nth(character)
        .map_or(text.len(), |(i, _)| i)
}

pub fn format(text: &mut String, selection: Range<usize>, cmd: Format) -> Range<usize> {
    let start = byte_at(text, selection.start);
    let end = byte_at(text, selection.end);
    match cmd {
        Format::Wrap(mark) => {
            text.insert_str(end, mark);
            text.insert_str(start, mark);
            selection.start + mark.len()..selection.end + mark.len()
        }
        Format::Block(_) | Format::Task => {
            let pos = text[..start].rfind(['\r', '\n']).map_or(0, |i| i + 1);
            let line = &text[pos..];
            let (n, prefix) = match cmd {
                Format::Block(p) => (block_prefix(line), p),
                _ => task_prefix(line),
            };
            let next = text[..pos].chars().count() + prefix.len();
            text.replace_range(pos..pos + n, prefix);
            next..next
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn edit_save_and_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "\u{feff}🦀 café\r\n").unwrap();
        let mut doc = Document::open(&path).unwrap();
        assert_eq!(format(&mut doc.text, 2..6, Format::Wrap("**")), 4..8);
        format(&mut doc.text, 0..0, Format::Block("## "));
        doc.save(&path).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "\u{feff}## 🦀 **café**\r\n"
        );
        fs::write(&path, "external edit").unwrap();
        assert!(doc.save(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external edit");
    }
}
