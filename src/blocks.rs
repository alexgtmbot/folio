//! Top-level CommonMark nodes mapped to exact byte ranges in the original file.
use pulldown_cmark::{Event, Options, Parser};
use std::ops::Range;

pub fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_DEFINITION_LIST
}

pub fn ranges(text: &str) -> Vec<Range<usize>> {
    let parser = Parser::new_ext(text, options());
    let mut starts: Vec<_> = parser
        .reference_definitions()
        .iter()
        .map(|(_, def)| def.span.start)
        .collect();
    let mut depth = 0;
    for (event, r) in parser.into_offset_iter() {
        match event {
            Event::Start(_) => {
                if depth == 0 {
                    starts.push(r.start);
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            _ if depth == 0 => starts.push(r.start),
            _ => {}
        }
    }
    if text.ends_with("\n\n") {
        starts.push(text.len());
    }
    starts.push(0);
    starts.sort_unstable();
    starts.dedup();
    starts.push(text.len());
    starts.windows(2).map(|w| w[0]..w[1]).collect()
}

// A following block can refer to definitions anywhere in the document.
pub fn with_references(text: &str, r: &Range<usize>, definitions: &[Range<usize>]) -> String {
    let mut block = text[r.clone()].to_owned();
    for def in definitions.iter().filter(|d| !r.contains(&d.start)) {
        block.push_str("\n\n");
        block.push_str(&text[def.clone()]);
    }
    block
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn blocks_cover_original_bytes() {
        for input in [
            "",
            "# Héllo\n\n> outer\n> - inner\n\n---\n\n[id]: /url\n",
            "Title\n===\n\n\n",
        ] {
            let blocks = ranges(input);
            assert_eq!(
                blocks.iter().map(|r| &input[r.clone()]).collect::<String>(),
                input
            );
        }
    }
}
