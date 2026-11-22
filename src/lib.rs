//! File fidelity and editing rules, independent of the window system.
use std::ops::Range;

#[derive(Default, Clone, Copy)]
pub struct FileFormat {
    bom: bool,
    crlf: bool,
}

impl FileFormat {
    pub fn decode(bytes: &[u8]) -> Result<(String, Self), std::str::Utf8Error> {
        let text = std::str::from_utf8(bytes)?;
        let bom = text.starts_with('\u{feff}');
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let crlf = text.contains("\r\n") && !text.replace("\r\n", "").contains('\n');
        let content = if crlf {
            text.replace("\r\n", "\n")
        } else {
            text.to_owned()
        };
        Ok((content, Self { bom, crlf }))
    }

    pub fn encode(self, text: &str) -> Vec<u8> {
        let mut out = String::new();
        if self.bom {
            out.push('\u{feff}');
        }
        if self.crlf {
            out.push_str(&text.replace('\n', "\r\n"));
        } else {
            out.push_str(text);
        }
        out.into_bytes()
    }
}

pub fn block_prefix(line: &str) -> usize {
    let hashes = line.bytes().take_while(|&c| c == b'#').count();
    if (1..=6).contains(&hashes) && line.as_bytes().get(hashes) == Some(&b' ') {
        hashes + 1
    } else if ["> ", "- ", "* "].iter().any(|p| line.starts_with(p)) {
        2
    } else {
        0
    }
}

pub fn task_prefix(line: &str) -> (usize, &'static str) {
    if line.starts_with("- [ ] ") {
        (6, "- [x] ")
    } else if line.starts_with("- [x] ") || line.starts_with("- [X] ") {
        (6, "- [ ] ")
    } else {
        (if line.starts_with("- ") { 2 } else { 0 }, "- [ ] ")
    }
}

#[derive(Default)]
pub struct Fence(Option<(u8, usize)>);
impl Fence {
    pub fn contains(&mut self, line: &str) -> bool {
        let trimmed = line.trim();
        let marker = trimmed.as_bytes().first().copied().unwrap_or(0);
        let count = trimmed.bytes().take_while(|&b| b == marker).count();
        if let Some((ch, n)) = self.0 {
            if marker == ch && count >= n && count == trimmed.len() {
                self.0 = None;
            }
            return true;
        }
        if matches!(marker, b'`' | b'~') && count >= 3 {
            self.0 = Some((marker, count));
            return true;
        }
        false
    }
}

// AppKit selections count UTF-16 units, while Rust strings use UTF-8 byte offsets.
pub fn byte_at(text: &str, utf16: usize) -> usize {
    let mut units = 0;
    for (i, ch) in text.char_indices() {
        if units + ch.len_utf16() > utf16 {
            return i;
        }
        units += ch.len_utf16();
    }
    text.len()
}

pub fn inline(line: &str, mut visit: impl FnMut(Range<usize>, &str)) {
    let mut pos = 0;
    while pos < line.len() {
        let rest = &line[pos..];
        let first = rest.chars().next().unwrap();
        let prev = line[..pos].chars().next_back();
        if !matches!(prev, Some('\\' | '*' | '_'))
            && let Some(mark) = ["**", "__", "`", "*", "_"]
                .into_iter()
                .find(|m| rest.starts_with(m))
        {
            let body = &rest[mark.len()..];
            if !body.starts_with(char::is_whitespace)
                && let Some((end, _)) = body
                    .match_indices(mark)
                    .find(|(i, _)| *i > 0 && !body[..*i].ends_with(char::is_whitespace))
            {
                let stop = pos + mark.len() * 2 + end;
                visit(pos..stop, mark);
                pos = stop;
                continue;
            }
        }
        pos += first.len_utf8();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn file_fidelity() {
        for s in ["", "\u{feff}# Héllo\r\n", "a\nb\r\nc", "文🦀"] {
            let (text, format) = FileFormat::decode(s.as_bytes()).unwrap();
            assert_eq!(format.encode(&text), s.as_bytes());
        }
        assert!(FileFormat::decode(&[0xff]).is_err());
    }
    #[test]
    fn markdown_and_unicode() {
        for (s, n) in [
            ("# Title", 2),
            ("###### 六", 7),
            ("####### x", 0),
            ("#tag", 0),
            ("- item", 2),
            ("* item", 2),
            ("> quote", 2),
            ("  # code", 0),
        ] {
            assert_eq!(block_prefix(s), n);
        }
        let s = "🦀 **café** and `六`";
        assert_eq!(byte_at(s, 2), 4);
        let mut spans = Vec::new();
        inline(s, |r, _| spans.push(&s[r]));
        assert_eq!(spans, ["**café**", "`六`"]);
        let mut fence = Fence::default();
        for (line, code) in [
            ("````rust", true),
            ("# inside", true),
            ("```", true),
            ("````", true),
            ("# outside", false),
        ] {
            assert_eq!(fence.contains(line), code);
        }
        assert_eq!(task_prefix("- [X] done"), (6, "- [ ] "));
    }
}
