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
        aliases: &["ul", "unordered"],
        template: "- ◇",
    },
    Command {
        name: "Numbered list",
        aliases: &["ol", "ordered"],
        template: "1. ◇",
    },
    Command {
        name: "Task",
        aliases: &["todo", "checkbox", "checklist"],
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
    Command {
        name: "Divider",
        aliases: &["rule", "separator", "hr"],
        template: "---\n◇",
    },
    Command {
        name: "Footnote",
        aliases: &["note"],
        template: "[^1]: ◇",
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

pub fn query(text: &str, selection: &Range<usize>) -> Option<Range<usize>> {
    if !selection.is_empty() {
        return None;
    }
    let end = byte_at(text, selection.end);
    let line = text[..end]
        .rsplit('\n')
        .next()?
        .trim_start_matches([' ', '\t']);
    let query = line.strip_prefix('/')?;
    if !query.chars().all(|c| c.is_ascii_alphanumeric() || c == ' ') {
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

pub fn insert(text: &mut String, range: Range<usize>, command: &Command) -> usize {
    let (before, after) = command
        .template
        .split_once('◇')
        .expect("command insertion marker");
    let caret = text[..range.start].chars().count() + before.chars().count();
    text.replace_range(range, &format!("{before}{after}"));
    caret
}

#[cfg(test)]
mod tests {
    use super::*;
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
