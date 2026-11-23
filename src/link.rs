//! Compose links and images from plain text fields.
use eframe::egui;
use std::ops::Range;

/// Link a selection in ordinary prose; other paste contexts keep their normal editor behavior.
pub fn paste_over_selection(source: &str, range: Range<usize>, clipboard: &str) -> Option<String> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let destination = clipboard.trim();
    if range.is_empty()
        || destination
            .chars()
            .any(|c| c.is_whitespace() || c.is_control())
    {
        return None;
    }
    let url = url::Url::parse(destination).ok()?;
    if !matches!(url.scheme(), "http" | "https") || url.host().is_none() {
        return None;
    }
    let mut excluded = 0;
    for (event, span) in Parser::new_ext(source, crate::blocks::options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { .. } | Tag::Image { .. } | Tag::CodeBlock(_)) => excluded += 1,
            Event::End(TagEnd::Link | TagEnd::Image | TagEnd::CodeBlock) => excluded -= 1,
            Event::Text(value)
                if excluded == 0
                    && span.start <= range.start
                    && range.end <= span.end
                    && value.as_ref() == &source[span] =>
            {
                let mut link = Link::new(0, range.clone(), source[range].to_owned(), false);
                link.destination = destination.to_owned();
                return Some(link.markdown());
            }
            _ => {}
        }
    }
    None
}

/// Resolve a web link under the caret, including reference-style links.
pub fn web_link_at(source: &str, byte: usize) -> Option<String> {
    use pulldown_cmark::{Event, Parser, Tag};
    Parser::new_ext(source, crate::blocks::options())
        .into_offset_iter()
        .find_map(|(event, span)| {
            let Event::Start(Tag::Link { dest_url, .. }) = event else {
                return None;
            };
            if !span.contains(&byte) {
                return None;
            }
            let url = url::Url::parse(&dest_url).ok()?;
            (matches!(url.scheme(), "https" | "http") && url.host().is_some()).then(|| url.into())
        })
}

struct ExistingLabel {
    destination: String,
    displayed: String,
    markdown: Option<String>,
    title: String,
}

impl ExistingLabel {
    fn unlinked_text(&self) -> String {
        if let Some(markdown) = &self.markdown {
            return markdown.clone();
        }
        // Autolinks have a plain-text label that may contain Markdown delimiters.
        let mut plain = String::new();
        for c in self.displayed.chars() {
            if c.is_ascii_punctuation() {
                plain.push('\\');
            }
            plain.push(c);
        }
        plain
    }
}

pub struct Link {
    pub tab: u64,
    pub range: Range<usize>,
    pub label: String,
    destination: String,
    image: bool,
    first_frame: bool,
    existing: Option<ExistingLabel>,
}

impl Link {
    pub fn new(tab: u64, range: Range<usize>, label: String, image: bool) -> Self {
        Self {
            tab,
            range,
            label,
            destination: String::new(),
            image,
            first_frame: true,
            existing: None,
        }
    }

    pub fn at_selection(tab: u64, source: &str, selection: Range<usize>, image: bool) -> Self {
        use pulldown_cmark::{Event, Parser, Tag};
        let mut selection = selection;
        let mut events = Parser::new_ext(source, crate::blocks::options()).into_offset_iter();
        while let Some((event, span)) = events.next() {
            let (dest, title) = match event {
                Event::Start(tag @ (Tag::Strong | Tag::Emphasis | Tag::Strikethrough))
                    if !image && span == selection =>
                {
                    let width = match tag {
                        Tag::Strong => 2,
                        Tag::Strikethrough => source[span.clone()]
                            .bytes()
                            .take_while(|&b| b == b'~')
                            .count(),
                        _ => 1,
                    };
                    selection = span.start + width..span.end - width;
                    continue;
                }
                Event::Start(Tag::Link {
                    dest_url, title, ..
                }) if !image => (dest_url, title),
                Event::Start(Tag::Image {
                    dest_url, title, ..
                }) if image => (dest_url, title),
                _ => continue,
            };
            if !span.contains(&selection.start) || selection.end > span.end {
                continue;
            }
            let mut label = String::new();
            let mut depth = 1;
            let mut last = span.start;
            for (event, child) in events.by_ref() {
                match event {
                    Event::End(_) => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    Event::Start(_) => depth += 1,
                    Event::Text(value) | Event::Code(value) => label.push_str(&value),
                    Event::SoftBreak | Event::HardBreak => label.push(' '),
                    _ => {}
                }
                last = child.end;
            }
            let raw = &source[span.clone()];
            let prefix = if image { "![" } else { "[" };
            let original = raw
                .strip_prefix(prefix)
                .and_then(|rest| {
                    let after = last.saturating_sub(span.start + prefix.len());
                    Some(&rest[..after + rest.get(after..)?.find(']')?])
                })
                .map(str::to_owned);
            let mut dialog = Self::new(tab, span, label, image);
            dialog.destination = dest.into_string();
            dialog.existing = Some(ExistingLabel {
                destination: dialog.destination.clone(),
                displayed: dialog.label.clone(),
                markdown: original,
                title: title.into_string(),
            });
            return dialog;
        }
        Self::new(tab, selection.clone(), source[selection].to_owned(), image)
    }

    pub fn image_markdown(label: &str, path: &str) -> String {
        let mut image = Self::new(0, 0..0, label.to_owned(), true);
        image.destination = path.to_owned();
        image.markdown()
    }
    fn markdown(&self) -> String {
        let mut label = String::new();
        for c in self.label.chars() {
            if matches!(
                c,
                '\\' | '[' | ']' | '*' | '_' | '`' | '<' | '>' | '&' | '~' | '|'
            ) {
                label.push('\\');
            }
            label.push(c);
        }
        let mut dest = String::new();
        for c in self.destination.trim().chars() {
            match c {
                ' ' => dest.push_str("%20"),
                '<' => dest.push_str("%3C"),
                '>' => dest.push_str("%3E"),
                '|' => dest.push_str("%7C"),
                '\\' | '(' | ')' | '&' => {
                    dest.push('\\');
                    dest.push(c);
                }
                _ => dest.push(c),
            }
        }
        if let Some(existing) = &self.existing {
            if existing.displayed == self.label
                && let Some(raw) = &existing.markdown
            {
                label.clone_from(raw);
            }
            if !existing.title.is_empty() {
                dest.push_str(" \"");
                for c in existing.title.chars() {
                    if c == '|' {
                        dest.push_str("&#124;");
                        continue;
                    }
                    if matches!(c, '\\' | '"' | '&') {
                        dest.push('\\');
                    }
                    dest.push(c);
                }
                dest.push('"');
            }
        }
        format!("{}[{label}]({dest})", if self.image { "!" } else { "" })
    }

    fn apply(&mut self) -> Option<String> {
        if self.existing.as_ref().is_some_and(|original| {
            self.label == original.displayed && self.destination == original.destination
        }) {
            return None;
        }
        if self.existing.is_none() && !self.image && self.label.is_empty() {
            self.label = self.destination.trim().to_owned();
        }
        Some(self.markdown())
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Option<String>> {
        let modal = egui::Modal::new("insert link".into()).show(ctx, |ui| {
            ui.set_min_width(310.);
            ui.heading(if self.existing.is_some() {
                if self.image {
                    "Edit image"
                } else {
                    "Edit link"
                }
            } else if self.image {
                "Insert image"
            } else {
                "Insert link"
            });
            ui.add_space(8.);
            ui.label(if self.image {
                "Alternative text"
            } else {
                "Link text"
            });
            let label = ui.add(
                egui::TextEdit::singleline(&mut self.label)
                    .desired_width(f32::INFINITY)
                    .hint_text("Display text"),
            );
            ui.label(if self.image {
                "Image path"
            } else {
                "URL or file path"
            });
            let url = ui.add(
                egui::TextEdit::singleline(&mut self.destination)
                    .desired_width(f32::INFINITY)
                    .hint_text(if self.image {
                        "images/photo.png"
                    } else {
                        "https://example.com"
                    }),
            );
            ui.add_space(8.);
            if self.first_frame {
                if self.label.is_empty() {
                    label.request_focus()
                } else {
                    url.request_focus()
                }
                self.first_frame = false;
            }
            if label.lost_focus()
                && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
            {
                url.request_focus();
            }
            let ready = !self.destination.trim().is_empty()
                && !self.destination.chars().any(char::is_control);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        ready,
                        egui::Button::new(if self.existing.is_some() {
                            "Apply"
                        } else {
                            "Insert"
                        }),
                    )
                    .clicked()
                    || ready
                        && url.lost_focus()
                        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                {
                    return Some(self.apply());
                }
                if !self.image
                    && let Some(existing) = &self.existing
                    && ui.button("Remove link").clicked()
                {
                    return Some(Some(existing.unlinked_text()));
                }
                ui.button("Cancel").clicked().then_some(None)
            })
            .inner
        });
        if modal.should_close() {
            Some(None)
        } else {
            modal.inner
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn linking_a_whole_emphasized_selection_keeps_its_formatting() {
        for (source, expected) in [
            ("**Café**", "**[Café](guide.md)**"),
            ("_**Café**_", "_**[Café](guide.md)**_"),
            ("~~Café~~", "~~[Café](guide.md)~~"),
        ] {
            let mut link = Link::at_selection(0, source, 0..source.len(), false);
            assert_eq!(link.label, "Café");
            link.destination = "guide.md".into();
            let mut result = source.to_owned();
            result.replace_range(link.range.clone(), &link.apply().unwrap());
            assert_eq!(result, expected);
        }
    }

    #[test]
    fn editing_a_table_link_preserves_pipes_in_its_title() {
        use pulldown_cmark::{Event, Parser, Tag};
        let source = "| Guide |\n| --- |\n| [Map](old.md \"North &#124; South\") |";
        let caret = source.find("Map").unwrap();
        let mut link = Link::at_selection(0, source, caret..caret, false);
        link.destination = "new.md".into();
        let mut edited = source.to_owned();
        edited.replace_range(link.range.clone(), &link.markdown());
        assert!(
            Parser::new_ext(&edited, crate::blocks::options()).any(|event| {
                matches!(event, Event::Start(Tag::Link { dest_url, title, .. })
                if dest_url.as_ref() == "new.md" && title.as_ref() == "North | South")
            })
        );
    }

    #[test]
    fn removing_a_link_preserves_inline_formatting_and_reference_definitions() {
        let source = "See [the **map**][route].\n\n[route]: guide.md";
        let dialog = Link::at_selection(0, source, 6..6, false);
        let mut text = source.to_owned();
        text.replace_range(dialog.range, &dialog.existing.unwrap().unlinked_text());
        assert_eq!(text, "See the **map**.\n\n[route]: guide.md");

        let url = "https://example.org/a*b*c";
        let dialog = Link::at_selection(0, &format!("<{url}>"), 1..1, false);
        let mut html = String::new();
        pulldown_cmark::html::push_html(
            &mut html,
            pulldown_cmark::Parser::new(&dialog.existing.unwrap().unlinked_text()),
        );
        assert_eq!(html, format!("<p>{url}</p>\n"));
    }

    #[test]
    fn applying_unchanged_links_leaves_original_source_untouched() {
        for (source, image) in [
            ("[guide][map]\n\n[map]: old.md 'Title'", false),
            ("![map](<a b.png> 'Trail')", true),
            ("<https://example.org>", false),
        ] {
            let mut dialog = Link::at_selection(0, source, 1..1, image);
            assert!(dialog.apply().is_none());
            dialog.destination = "new.md".into();
            assert!(dialog.apply().is_some());
        }
    }

    #[test]
    fn editing_a_link_preserves_its_formatted_label_and_title() {
        let source = "See [the **map**](old.md \"Trail guide\") today";
        let mut dialog = Link::at_selection(3, source, 12..12, false);
        assert_eq!(dialog.label, "the map");
        assert_eq!(dialog.destination, "old.md");
        dialog.destination = "new map.md".into();
        let mut updated = source.to_owned();
        updated.replace_range(dialog.range.clone(), &dialog.markdown());
        assert_eq!(
            updated,
            "See [the **map**](new%20map.md \"Trail guide\") today"
        );
    }

    #[test]
    fn follows_web_references_under_the_caret_only() {
        let source = "🦀 [docs][ref] and `[literal](https://example.org)`\n\n[ref]: https://example.org/docs";
        assert_eq!(
            web_link_at(source, source.find("docs").unwrap()).as_deref(),
            Some("https://example.org/docs")
        );
        assert!(web_link_at(source, source.find("literal").unwrap()).is_none());
        assert!(web_link_at("[local](file:///tmp/note)", 2).is_none());
    }

    #[test]
    fn paste_url_keeps_unicode_label_and_respects_literal_contexts() {
        let url = "https://example.org/map_(north)";
        let md = "**café**";
        assert_eq!(
            paste_over_selection(md, 2..7, url).unwrap(),
            r"[café](https://example.org/map_\(north\))"
        );
        for (source, range) in [
            ("`abc`", 1..4),
            ("[abc](x)", 1..4),
            ("abc", 0..0),
            ("~~~\nabc\n~~~", 4..7),
        ] {
            assert!(paste_over_selection(source, range, url).is_none());
        }
        for paste in [
            "javascript:alert(1)",
            "two words",
            "https://",
            "https://a.test extra",
        ] {
            assert!(paste_over_selection("abc", 0..3, paste).is_none());
        }
    }
    #[test]
    fn plain_fields_round_trip_through_commonmark() {
        for image in [false, true] {
            let mut link = Link::new(0, 0..0, "a [label] &amp; *bold*".into(), image);
            link.destination = "notes/a (draft).md?x=1&copy;".into();
            let md = link.markdown();
            use pulldown_cmark::{Event, Parser, Tag};
            let mut events = Parser::new(&md);
            let dest = events
                .find_map(|e| match e {
                    Event::Start(Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. }) => {
                        Some(dest_url)
                    }
                    _ => None,
                })
                .unwrap();
            assert_eq!(&*dest, "notes/a%20(draft).md?x=1&copy;");
            assert_eq!(
                events
                    .filter_map(|e| if let Event::Text(t) = e {
                        Some(t.into_string())
                    } else {
                        None
                    })
                    .collect::<String>(),
                link.label
            );
        }
    }
}
