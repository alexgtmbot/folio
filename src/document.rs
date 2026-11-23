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
    pub fn refresh_dirty(&mut self) {
        self.dirty = self.format.encode(&self.text) != self.last_saved;
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

/// Move beyond the closing delimiter when toggling the style at its trailing edge.
pub fn exit_inline(text: &str, selection: &Range<usize>, cmd: Format) -> Option<Range<usize>> {
    use pulldown_cmark::{Event, Parser, TagEnd};
    let Format::Wrap(mark) = cmd else { return None };
    if !selection.is_empty() {
        return None;
    }
    let caret = byte_at(text, selection.start);
    if !matches!(text.as_bytes().get(caret), Some(b'*' | b'_' | b'~' | b'`')) {
        return None;
    }
    for (event, span) in Parser::new_ext(text, crate::blocks::options()).into_offset_iter() {
        if !(span.start < caret && caret < span.end) {
            continue;
        }
        let suffix = &text[caret..span.end];
        let matches = match (mark, event) {
            ("**", Event::End(TagEnd::Strong)) => matches!(suffix, "**" | "__"),
            ("*", Event::End(TagEnd::Emphasis)) => matches!(suffix, "*" | "_"),
            ("~~", Event::End(TagEnd::Strikethrough)) => matches!(suffix, "~~" | "~"),
            ("`", Event::Code(_)) => {
                suffix.bytes().all(|b| b == b'`')
                    && suffix.len()
                        == text[span.start..span.end]
                            .bytes()
                            .take_while(|b| *b == b'`')
                            .count()
            }
            _ => false,
        };
        if matches {
            let after = selection.start + suffix.len();
            return Some(after..after);
        }
    }
    None
}

pub fn format(text: &mut String, selection: Range<usize>, cmd: Format) -> Range<usize> {
    let start = byte_at(text, selection.start);
    let end = byte_at(text, selection.end);
    match cmd {
        Format::Wrap(mark) => {
            if text[..start].ends_with(mark) && text[end..].starts_with(mark) {
                text.replace_range(end..end + mark.len(), "");
                text.replace_range(start - mark.len()..start, "");
                return selection.start - mark.len()..selection.end - mark.len();
            }
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

/// Return the character selection and replacement for Enter on a Markdown list line.
pub fn list_enter(text: &str, caret: usize) -> Option<(Range<usize>, String)> {
    use pulldown_cmark::{Event, Parser, Tag};
    let end = byte_at(text, caret);
    let start = text[..end].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[start..end];
    let body = before.trim_start_matches([' ', '\t']);
    let indent = &before[..before.len() - body.len()];
    let (marker, content) = body.split_once(' ')?;
    let next = match marker {
        "-" | "+" | "*" => marker.to_owned(),
        _ => {
            let digits = marker.strip_suffix(['.', ')'])?;
            if digits.is_empty() || digits.len() > 9 || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            format!(
                "{}{}",
                digits.parse::<u32>().ok()?.checked_add(1)?,
                &marker[digits.len()..]
            )
        }
    };
    if !Parser::new(text).into_offset_iter().any(|(e, r)| {
        matches!(e, Event::Start(Tag::Item)) && (start..=start + indent.len()).contains(&r.start)
    }) {
        return None;
    }
    let task =
        content.starts_with("[ ] ") || content.starts_with("[x] ") || content.starts_with("[X] ");
    let empty = if task { &content[4..] } else { content };
    if empty.trim().is_empty() && text[end..].split('\n').next()?.trim().is_empty() {
        return Some((caret - before.chars().count()..caret, String::new()));
    }
    Some((
        caret..caret,
        format!("\n{indent}{next} {}", if task { "[ ] " } else { "" }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enter_handles_list_types_and_exits_empty_items() {
        for (line, next) in [
            ("- Hello", "\n- "),
            ("2. Yo", "\n3. "),
            ("- [x] café", "\n- [ ] "),
            ("- parent\n  * child", "\n  * "),
        ] {
            let n = line.chars().count();
            assert_eq!(list_enter(line, n), Some((n..n, next.into())));
        }
        assert_eq!(list_enter("- ", 2), Some((0..2, String::new())));
        assert_eq!(list_enter("```\n- literal", 13), None);
    }
    #[test]
    fn toggle_off_after_typing_leaves_delimiters_intact() {
        for (mark, source) in [
            ("**", "__🦀__"),
            ("*", "*🦀*"),
            ("~~", "~~🦀~~"),
            ("`", "``🦀``"),
        ] {
            let n = source.find('🦀').unwrap() + 1;
            let end = source.chars().count();
            assert_eq!(
                exit_inline(source, &(n..n), Format::Wrap(mark)),
                Some(end..end)
            );
        }
        assert_eq!(
            exit_inline("x **literal", &(2..2), Format::Wrap("**")),
            None
        );
    }

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
