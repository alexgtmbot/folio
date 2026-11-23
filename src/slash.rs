use crate::document::byte_at;
use std::ops::Range;

pub struct Command {
    pub name: &'static str,
    pub template: &'static str,
}

// The diamond marks the insertion point after choosing a command.
pub const TABLE: Command = Command {
    name: "Table",
    template: "| Column | Value |\n| --- | --- |\n| ◇ |   |",
};

pub const COMMANDS: &[Command] = &[
    Command {
        name: "Heading 1",
        template: "# ◇",
    },
    Command {
        name: "Heading 2",
        template: "## ◇",
    },
    Command {
        name: "Heading 3",
        template: "### ◇",
    },
    Command {
        name: "Bullet list",
        template: "- ◇",
    },
    Command {
        name: "Numbered list",
        template: "1. ◇",
    },
    Command {
        name: "Task",
        template: "- [ ] ◇",
    },
    Command {
        name: "Quote",
        template: "> ◇",
    },
    Command {
        name: "Code block",
        template: "```\n◇\n```",
    },
    TABLE,
    Command {
        name: "Link",
        template: "[◇](https://)",
    },
    Command {
        name: "Image",
        template: "![◇](image.png)",
    },
    Command {
        name: "Divider",
        template: "---\n◇",
    },
    Command {
        name: "Footnote",
        template: "[^1]: ◇",
    },
    Command {
        name: "Heading 4",
        template: "#### ◇",
    },
    Command {
        name: "Heading 5",
        template: "##### ◇",
    },
    Command {
        name: "Heading 6",
        template: "###### ◇",
    },
    Command {
        name: "Bold",
        template: "**◇**",
    },
    Command {
        name: "Italic",
        template: "*◇*",
    },
    Command {
        name: "Strikethrough",
        template: "~~◇~~",
    },
    Command {
        name: "Inline code",
        template: "`◇`",
    },
    Command {
        name: "Definition list",
        template: "Term\n: ◇",
    },
    Command {
        name: "Reference link",
        template: "[◇][ref]\n\n[ref]: https://",
    },
];

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
