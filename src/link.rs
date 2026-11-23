//! Compose links and images from plain text fields.
use eframe::egui;
use std::ops::Range;

pub struct Link {
    pub tab: u64,
    pub range: Range<usize>,
    pub label: String,
    destination: String,
    image: bool,
    first_frame: bool,
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
        }
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
        format!("{}[{label}]({dest})", if self.image { "!" } else { "" })
    }

    pub fn show(&mut self, ctx: &egui::Context) -> Option<Option<String>> {
        let modal = egui::Modal::new("insert link".into()).show(ctx, |ui| {
            ui.set_min_width(310.);
            ui.heading(if self.image {
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
                if ui.add_enabled(ready, egui::Button::new("Insert")).clicked()
                    || ready
                        && url.lost_focus()
                        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                {
                    if !self.image && self.label.is_empty() {
                        self.label = self.destination.trim().to_owned();
                    }
                    return Some(Some(self.markdown()));
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
