use crate::document::byte_at;
use std::ops::Range;

pub struct Command {
    pub name: &'static str,
    pub template: &'static str,
    aliases: &'static [&'static str],
}

// The diamond marks the insertion point after choosing a command.
pub const TABLE: Command = Command {
    name: "Table",
    aliases: &["grid"],
    template: "| Column | Value |\n| --- | --- |\n| ◇ |   |",
};

pub const DIVIDER: Command = Command {
    name: "Divider",
    aliases: &["rule", "separator", "hr"],
    template: "***\n◇",
};

pub const COMMANDS: &[Command] = &[
    Command {
        name: "Heading 1",
        aliases: &["h1", "title"],
        template: "# ◇",
    },
    Command {
        name: "Heading 2",
        aliases: &["h2", "subtitle"],
        template: "## ◇",
    },
    Command {
        name: "Heading 3",
        aliases: &["h3"],
        template: "### ◇",
    },
    Command {
        name: "Bullet list",
        aliases: &["ul", "unordered", "unordered list", "bulleted list"],
        template: "- ◇",
    },
    Command {
        name: "Numbered list",
        aliases: &["ol", "ordered", "ordered list"],
        template: "1. ◇",
    },
    Command {
        name: "Task",
        aliases: &["todo", "todo list", "to-do list", "checkbox", "checklist"],
        template: "- [ ] ◇",
    },
    Command {
        name: "Quote",
        aliases: &["blockquote"],
        template: "> ◇",
    },
    Command {
        name: "Code block",
        aliases: &["fence", "snippet"],
        template: "```\n◇\n```",
    },
    TABLE,
    Command {
        name: "Link",
        aliases: &["url", "hyperlink"],
        template: "[◇](https://)",
    },
    Command {
        name: "Image",
        aliases: &["photo", "picture"],
        template: "![◇](image.png)",
    },
    DIVIDER,
    Command {
        name: "Footnote",
        aliases: &["note"],
        template: "[^1]\n\n[^1]: ◇",
    },
    Command {
        name: "Heading 4",
        aliases: &["h4"],
        template: "#### ◇",
    },
    Command {
        name: "Heading 5",
        aliases: &["h5"],
        template: "##### ◇",
    },
    Command {
        name: "Heading 6",
        aliases: &["h6"],
        template: "###### ◇",
    },
    Command {
        name: "Bold",
        aliases: &["strong"],
        template: "**◇**",
    },
    Command {
        name: "Italic",
        aliases: &["emphasis"],
        template: "*◇*",
    },
    Command {
        name: "Strikethrough",
        aliases: &["strike", "delete"],
        template: "~~◇~~",
    },
    Command {
        name: "Inline code",
        aliases: &["monospace"],
        template: "`◇`",
    },
    Command {
        name: "Definition list",
        aliases: &["glossary"],
        template: "Term\n: ◇",
    },
    Command {
        name: "Reference link",
        aliases: &["ref"],
        template: "[◇][ref]\n\n[ref]: https://",
    },
];

pub fn matching(query: &str) -> impl Iterator<Item = &'static Command> + '_ {
    let query = query.trim().to_ascii_lowercase();
    let mut matches = Vec::new();
    for command in COMMANDS {
        let name = command.name.to_ascii_lowercase();
        let rank = std::iter::once(name.as_str())
            .chain(command.aliases.iter().copied())
            .filter_map(|label| {
                if label == query {
                    Some(0)
                } else if label.starts_with(&query) {
                    Some(1)
                } else if label.contains(&query) {
                    Some(2)
                } else {
                    None
                }
            })
            .min();
        if let Some(rank) = rank {
            matches.push((rank, command));
        }
    }
    matches.sort_by_key(|(rank, _)| *rank);
    matches.into_iter().map(|(_, command)| command)
}

pub(crate) fn split_quote_prefix(line: &str) -> (&str, &str) {
    let mut body = line;
    while let Some(after) = body.trim_start_matches([' ', '\t']).strip_prefix('>') {
        body = after.strip_prefix([' ', '\t']).unwrap_or(after);
    }
    line.split_at(line.len() - body.len())
}

pub(crate) fn list_content(line: &str) -> Option<(&str, usize)> {
    let body = line.trim_start_matches(' ');
    let (marker, rest) = body.split_once(' ')?;
    if !matches!(marker, "-" | "+" | "*") {
        let digits = marker.strip_suffix(['.', ')'])?;
        if !(1..=9).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
    }
    let rest = rest.trim_start_matches(' ');
    let indent = line.len() - rest.len();
    let content = ["[ ] ", "[x] ", "[X] "]
        .into_iter()
        .find_map(|prefix| rest.strip_prefix(prefix))
        .unwrap_or(rest);
    Some((content, indent))
}

pub fn query(text: &str, selection: &Range<usize>) -> Option<Range<usize>> {
    if !selection.is_empty() {
        return None;
    }
    let end = byte_at(text, selection.end);
    let line = text[..end]
        .rsplit('\n')
        .next()?
        .trim_start_matches([' ', '\t']);
    let (_, line) = split_quote_prefix(line);
    let line =
        list_content(line).map_or(line.trim_start_matches([' ', '\t']), |(content, _)| content);
    let line = if line.starts_with('#') {
        &line[folio::block_prefix(line)..]
    } else {
        line
    };
    let query = line.strip_prefix('/')?;
    if !query
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-'))
    {
        return None;
    }
    let start = end - line.len();
    let in_code = pulldown_cmark::Parser::new(text)
        .into_offset_iter()
        .any(|(event, r)| {
            matches!(
                event,
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::CodeBlock(_))
            ) && r.contains(&start)
        });
    (!in_code).then_some(start..end)
}

pub fn insert(text: &mut String, mut range: Range<usize>, command: &Command) -> usize {
    let prefix = text[..range.start].rsplit('\n').next().unwrap_or_default();
    let (quote, prefix) = split_quote_prefix(prefix);
    let (body, indent) = list_content(prefix).unwrap_or((prefix, 0));
    let heading = body.trim_start_matches([' ', '\t']);
    if text[range.clone()].starts_with('/')
        && heading.starts_with('#')
        && folio::block_prefix(heading) == heading.len()
    {
        range.start -= heading.len();
    }
    let generated;
    let template = if command.name == "Footnote" {
        // Reserve unresolved references too, so inserting a note cannot retarget them.
        let used: std::collections::HashSet<&str> = text
            .match_indices("[^")
            .filter_map(|(at, _)| {
                let tail = &text[at + 2..];
                let digits = tail.bytes().take_while(u8::is_ascii_digit).count();
                (digits > 0 && tail[digits..].starts_with(']')).then(|| &tail[..digits])
            })
            .collect();
        let number = (1usize..)
            .find(|n| !used.contains(n.to_string().as_str()))
            .unwrap();
        generated = command.template.replace("[^1]", &format!("[^{number}]"));
        &generated
    } else if command.name == "Reference link" {
        // A new definition would also activate any matching unresolved shortcut links.
        let used: std::collections::HashSet<_> = text
            .split('[')
            .skip(1)
            .filter_map(|part| part.split_once(']'))
            .map(|(label, _)| label.trim().to_ascii_lowercase())
            .collect();
        let label = std::iter::once("ref".to_owned())
            .chain((2usize..).map(|n| format!("ref{n}")))
            .find(|label| !used.contains(label))
            .unwrap();
        generated = command.template.replace("[ref]", &format!("[{label}]"));
        &generated
    } else {
        command.template
    };
    let template = template.replace('\n', &format!("\n{quote}{}", " ".repeat(indent)));
    let (before, after) = template.split_once('◇').expect("command insertion marker");
    let caret = text[..range.start].chars().count() + before.chars().count();
    text.replace_range(range, &format!("{before}{after}"));
    caret
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reference_commands_preserve_existing_destinations_and_unresolved_labels() {
        let mut text = "[Earlier][REF] and [ref2]\n\n[REF]: old.md\n\n/ref".to_owned();
        let end = text.len();
        let caret = insert(&mut text, end - 4..end, matching("ref").next().unwrap());
        text.insert_str(byte_at(&text, caret), "New");
        assert!(text.ends_with("[New][ref3]\n\n[ref3]: https://"));
        let links: Vec<_> = pulldown_cmark::Parser::new(&text)
            .filter_map(|event| match event {
                pulldown_cmark::Event::Start(pulldown_cmark::Tag::Link { dest_url, .. }) => {
                    Some(dest_url.into_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(links, ["old.md", "https://"]);
    }

    #[test]
    fn slash_can_restyle_an_empty_heading_without_stacking_markers() {
        let mut text = "🦀\n\n> ## /h3".to_owned();
        let end = text.chars().count();
        let range = query(&text, &(end..end)).unwrap();
        assert_eq!(&text[range.clone()], "/h3");
        let caret = insert(&mut text, range, matching("h3").next().unwrap());
        text.insert_str(byte_at(&text, caret), "Title");
        assert_eq!(text, "🦀\n\n> ### Title");
        let prose = "# Existing /h3";
        assert!(query(prose, &(prose.len()..prose.len())).is_none());
    }

    #[test]
    fn divider_after_prose_is_a_rule_and_keeps_the_caret_below_it() {
        let mut text = "Notes\n/divider".to_owned();
        let end = text.len();
        let caret = insert(&mut text, 6..end, matching("divider").next().unwrap());
        text.insert_str(byte_at(&text, caret), "Next");
        let events: Vec<_> = pulldown_cmark::Parser::new(&text).collect();
        assert!(events.contains(&pulldown_cmark::Event::Rule));
        assert_eq!(text, "Notes\n***\nNext");
    }

    #[test]
    fn quoted_commands_preserve_containers_on_every_inserted_line() {
        let mut text = "> > - /grid".to_owned();
        let end = text.len();
        let span = query(&text, &(end..end)).unwrap();
        let caret = insert(&mut text, span, &TABLE);
        assert_eq!(
            text,
            "> > - | Column | Value |\n> >   | --- | --- |\n> >   |  |   |"
        );
        assert_eq!(&text[byte_at(&text, caret)..], " |   |");
        let code = "> ```\n> /grid\n> ```";
        let caret = code.find("/grid").unwrap() + 5;
        assert!(query(code, &(caret..caret)).is_none());
    }

    #[test]
    fn footnote_command_pairs_a_fresh_reference_and_definition() {
        let mut text = "🦀[^1] and [^2]\n\n[^1]: original\n\n/foot".to_owned();
        let start = text.find("/foot").unwrap();
        let end = text.len();
        let caret = insert(&mut text, start..end, matching("footnote").next().unwrap());
        text.insert_str(byte_at(&text, caret), "new note");
        assert!(text.ends_with("[^3]\n\n[^3]: new note"));
        use pulldown_cmark::{Event, Parser, Tag};
        let events: Vec<_> = Parser::new_ext(&text, crate::blocks::options()).collect();
        assert!(
            events
                .iter()
                .any(|e| matches!(e, Event::FootnoteReference(n) if n.as_ref() == "3"))
        );
        assert!(
            events.iter().any(
                |e| matches!(e, Event::Start(Tag::FootnoteDefinition(n)) if n.as_ref() == "3")
            )
        );
    }

    #[test]
    fn notion_list_names_insert_editable_markdown() {
        for (name, expected) in [
            ("to-do list", "- [ ] Buy milk"),
            ("bulleted list", "- Buy milk"),
            ("ordered list", "1. Buy milk"),
        ] {
            let mut text = format!("/{name}");
            let end = text.len();
            let range = query(&text, &(end..end)).unwrap();
            let command = matching(&text[range.start + 1..range.end]).next().unwrap();
            let caret = insert(&mut text, range, command);
            text.insert_str(byte_at(&text, caret), "Buy milk");
            assert_eq!(text, expected);
        }
    }

    #[test]
    fn aliases_resolve_to_the_same_command_used_by_keyboard_and_pointer() {
        for (query, expected) in [
            ("H3", "Heading 3"),
            ("checkbox", "Task"),
            ("ordered", "Numbered list"),
            ("ord", "Numbered list"),
            ("grid", "Table"),
            ("snippet", "Code block"),
            ("picture", "Image"),
        ] {
            assert_eq!(matching(query).next().unwrap().name, expected);
        }
        assert_eq!(matching("").count(), COMMANDS.len());
        assert_eq!(matching("1 do x").count(), 0);
    }
    #[test]
    fn list_commands_keep_multiline_content_inside_the_item() {
        let mut text = "- café\n  1. [x] /grid".to_owned();
        let end = text.chars().count();
        let span = query(&text, &(end..end)).unwrap();
        let caret = insert(&mut text, span, &TABLE);
        assert_eq!(
            text,
            "- café\n  1. [x] | Column | Value |\n     | --- | --- |\n     |  |   |"
        );
        assert_eq!(&text[byte_at(&text, caret)..], " |   |");
        let mut depth = 0;
        let table_depth = pulldown_cmark::Parser::new_ext(&text, crate::blocks::options())
            .find_map(|event| {
                use pulldown_cmark::{Event, Tag, TagEnd};
                match event {
                    Event::Start(Tag::Item) => depth += 1,
                    Event::End(TagEnd::Item) => depth -= 1,
                    Event::Start(Tag::Table(_)) => return Some(depth),
                    _ => {}
                }
                None
            });
        assert_eq!(table_depth, Some(2));
        assert!(query("- literal /grid", &(15..15)).is_none());
    }

    #[test]
    fn replaces_only_trigger_and_preserves_suffix() {
        let mut text = "🦀\n  /tab suffix".to_owned();
        let span = query(&text, &(8..8)).unwrap();
        let pos = insert(&mut text, span, &COMMANDS[8]);
        assert!(text.starts_with("🦀\n  | Column | Value |"));
        assert!(text.ends_with(" suffix"));
        assert_eq!(&text[byte_at(&text, pos)..], " |   | suffix");
        assert!(query("a/b", &(3..3)).is_none());
        assert!(query("```sh\n/tab\n```", &(10..10)).is_none());
        for command in COMMANDS {
            assert_eq!(command.template.matches('◇').count(), 1);
        }
    }
}
