//! File fidelity and editing rules, independent of the window system.

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
    let after_hashes = &line[hashes..];
    if (1..=6).contains(&hashes)
        && (after_hashes.is_empty() || after_hashes.starts_with([' ', '\t', '\r', '\n']))
    {
        line.len() - after_hashes.trim_start_matches([' ', '\t']).len()
    } else if ["- [ ] ", "- [x] ", "- [X] "]
        .iter()
        .any(|p| line.starts_with(p))
    {
        6
    } else if let Some((marker, _)) = line.split_once(' ')
        && let Some(n) = marker.strip_suffix(['.', ')'])
        && (1..=9).contains(&n.len())
        && n.bytes().all(|b| b.is_ascii_digit())
    {
        marker.len() + 1
    } else if ["> ", "- ", "* ", "+ "].iter().any(|p| line.starts_with(p)) {
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
            ("##\t  Café", 5),
            ("###\nNext", 3),
            ("##", 2),
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
        assert_eq!(task_prefix("- [X] done"), (6, "- [ ] "));
    }
}
