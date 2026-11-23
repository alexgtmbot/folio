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

pub fn word_count(text: &str) -> usize {
    use pulldown_cmark::TagEnd;
    let mut count = 0;
    let mut in_word = false;
    for event in Parser::new_ext(text, options()) {
        match event {
            Event::Text(value)
            | Event::Code(value)
            | Event::Html(value)
            | Event::InlineHtml(value) => {
                for c in value.chars() {
                    let visible = !c.is_whitespace();
                    count += usize::from(visible && !in_word);
                    in_word = visible;
                }
            }
            Event::End(
                TagEnd::Emphasis
                | TagEnd::Strong
                | TagEnd::Strikethrough
                | TagEnd::Link
                | TagEnd::Image,
            )
            | Event::Start(_)
            | Event::FootnoteReference(_)
            | Event::TaskListMarker(_) => {}
            _ => in_word = false,
        }
    }
    count
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

/// Move complete top-level blocks, retaining internal Markdown and the file's
/// final newline. The selection follows its content in Unicode characters.
pub fn move_selection(
    text: &mut String,
    selection: &Range<usize>,
    up: bool,
) -> Option<Range<usize>> {
    let blocks = ranges(text);
    let index = |c| {
        let byte = crate::document::byte_at(text, c);
        blocks
            .partition_point(|r| r.end <= byte)
            .min(blocks.len() - 1)
    };
    let first = index(selection.start);
    let last = index(
        selection
            .end
            .saturating_sub(usize::from(!selection.is_empty())),
    );
    let neighbor = if up { first.checked_sub(1)? } else { last + 1 };
    let adjacent = blocks.get(neighbor)?;
    let moved = blocks.get(first)?.start..blocks.get(last)?.end;
    let a = if up { adjacent } else { &moved };
    let b = if up { &moved } else { adjacent };
    let body = |r: &Range<usize>| text[r.clone()].trim_end_matches(['\r', '\n']);
    let left = body(a);
    let right = body(b);
    if left.trim().is_empty() || right.trim().is_empty() {
        return None;
    }
    let newline = if text[a.start + left.len()..a.end].contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let separator = &text[a.start + left.len()..a.end];
    let separator = if separator.matches('\n').count() < 2 {
        newline.repeat(2)
    } else {
        separator.to_owned()
    };
    let new_start = a.start + if up { 0 } else { right.len() + separator.len() };
    let moved_len = body(&moved).chars().count();
    let old = text[..moved.start].chars().count();
    let suffix = &text[b.start + right.len()..b.end];
    let replacement = format!("{right}{separator}{left}{suffix}");
    let mut candidate = text.clone();
    candidate.replace_range(a.start..b.end, &replacement);
    // Moving an unclosed fence, for example, could swallow the next block.
    let after = ranges(&candidate);
    if after.len() != blocks.len() || !after.iter().any(|r| r.start == new_start) {
        return None;
    }
    let offset = candidate[..new_start].chars().count();
    let map = |pos: usize| offset + (pos - old).min(moved_len);
    *text = candidate;
    Some(map(selection.start)..map(selection.end))
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

/// Land on the neighboring block's first or last content line, past its block marker.
pub fn adjacent_caret(text: &str, block: Range<usize>, up: bool, column: usize) -> usize {
    let content = text[block.clone()].trim_end_matches(['\r', '\n']);
    let line = if up {
        content.rsplit('\n').next()
    } else {
        content.split('\n').next()
    }
    .unwrap_or("");
    let line_start = block.start + if up { content.len() - line.len() } else { 0 };
    let prefix = folio::block_prefix(line);
    text[..line_start + prefix].chars().count()
        + column.min(line[prefix..].trim_end_matches('\r').chars().count())
}

/// Markdown blank lines separate visual paragraphs. Remove the separator as
/// one edit, preserving the inline source on either side.
pub fn paragraph_separator(
    text: &str,
    left: &Range<usize>,
    right: &Range<usize>,
) -> Option<Range<usize>> {
    let paragraph = |r: &Range<usize>| {
        matches!(
            Parser::new_ext(&text[r.clone()], options()).next(),
            Some(Event::Start(pulldown_cmark::Tag::Paragraph))
        )
    };
    if !paragraph(left) || !(right.is_empty() || paragraph(right)) {
        return None;
    }
    let end = left.start + text[left.clone()].trim_end_matches(['\r', '\n']).len();
    (end < right.start).then_some(end..right.start)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn word_count_uses_document_text_across_inline_styles_and_cells() {
        assert_eq!(
            word_count("| Name | Status |\n| --- | --- |\n| Alpine | Ready |"),
            4
        );
        assert_eq!(
            word_count("# Note\n\n- [x] pre**fix** and [two words](https://example.org)\n\n---"),
            5
        );
        assert_eq!(word_count("a\u{202f}b\n\n`two words`"), 4);
        assert_eq!(word_count("---\n\n[ref]: https://example.org"), 0);
    }

    #[test]
    fn moving_up_from_eof_and_moving_a_range() {
        let mut text = "one\n\ntwo\n\nthree".to_owned();
        let end = text.chars().count();
        assert_eq!(move_selection(&mut text, &(end..end), true), Some(10..10));
        assert_eq!(text, "one\n\nthree\n\ntwo");
        let end = text.chars().count();
        assert_eq!(move_selection(&mut text, &(5..end), true), Some(0..10));
        assert_eq!(text, "three\n\ntwo\n\none");
    }
    #[test]
    fn moving_blocks_keeps_selection_history_and_saved_source() {
        let original = "# Header\nfirst 🦀\n\n> quote";
        let mut text = original.to_owned();
        let mut history = crate::history::History::new(&text);
        let at = text[..text.find('🦀').unwrap()].chars().count();
        let selected = move_selection(&mut text, &(at..at + 1), false).unwrap();
        assert_eq!(text, "# Header\n> quote\n\nfirst 🦀");
        assert_eq!(text.chars().nth(selected.start), Some('🦀'));
        history.record(&text);
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        std::fs::write(&path, original).unwrap();
        let mut doc = crate::document::Document::open(&path).unwrap();
        doc.text = text.clone();
        doc.save(&path).unwrap();
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
        history.restore(&mut text, false).unwrap();
        assert_eq!(text, original);
        assert!(move_selection(&mut text, &(0..0), true).is_none());
    }
    #[test]
    fn join_paragraphs_preserves_inline_source_and_leaves_structural_blocks_alone() {
        let mut text = "**été**\n\nsecond\n\n> quote".to_owned();
        let blocks = ranges(&text);
        assert!(paragraph_separator(&text, &blocks[1], &blocks[2]).is_none());
        let cut = paragraph_separator(&text, &blocks[0], &blocks[1]).unwrap();
        text.replace_range(cut, "");
        assert_eq!(text, "**été**second\n\n> quote");
    }
    #[test]
    fn vertical_navigation_lands_in_content() {
        let text = "# 🦀\n\nfirst\nsecond\n\n";
        let r = ranges(text);
        assert_eq!(adjacent_caret(text, r[0].clone(), true, 10), 3);
        assert_eq!(adjacent_caret(text, r[1].clone(), false, 2), 7);
        assert_eq!(adjacent_caret(text, r[1].clone(), true, 2), 13);
    }
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
