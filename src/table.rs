//! Edit a single GFM table while preserving the surrounding document.
use eframe::egui;
use pulldown_cmark::{Event, Parser, Tag};
use std::ops::Range;

struct Table {
    rows: Vec<Vec<Range<usize>>>,
    end: usize,
    start: usize,
}

impl Table {
    fn parse(text: &str) -> Option<Self> {
        let mut events = Parser::new_ext(text, crate::blocks::options()).into_offset_iter();
        let (Event::Start(Tag::Table(_)), span) = events.next()? else {
            return None;
        };
        if !text[span.end..].trim().is_empty() {
            return None;
        }
        let mut rows = Vec::<Vec<_>>::new();
        for (event, range) in events {
            match event {
                Event::Start(Tag::TableHead | Tag::TableRow) => rows.push(Vec::new()),
                Event::Start(Tag::TableCell) => rows.last_mut()?.push(range),
                _ => {}
            }
        }
        Some(Self {
            rows,
            start: span.start,
            end: text[..span.end].trim_end_matches(['\r', '\n']).len(),
        })
    }
}

pub fn edit(
    ui: &mut egui::Ui,
    text: &mut String,
    focus: Option<Range<usize>>,
    size: f32,
) -> Option<(bool, Option<Range<usize>>)> {
    let table = Table::parse(text)?;
    let mut replacement = None;
    let mut selection = None;
    let cols = table.rows.first()?.len().max(1);
    let width = ((ui.available_width() - 8. * cols as f32) / cols as f32).max(45.);
    egui::ScrollArea::horizontal().show(ui, |ui| {
        for (row, cells) in table.rows.iter().enumerate() {
            ui.horizontal(|ui| {
                for (col, span) in cells.iter().enumerate() {
                    let original = &text[span.clone()];
                    let raw = original.trim();
                    let start = if raw.is_empty() && !span.is_empty() {
                        span.start + 1
                    } else {
                        span.start + original.len() - original.trim_start().len()
                    };
                    let mut value = raw.replace("\\|", "|");
                    let mut layout = |ui: &egui::Ui, s: &str, width: f32| {
                        ui.fonts(|f| {
                            f.layout_job(crate::window::layout(
                                s,
                                width,
                                ui.visuals().text_color(),
                                false,
                                0,
                                size,
                            ))
                        })
                    };
                    let id = ui.make_persistent_id(("cell", row, col));
                    if let Some(r) = &focus
                        && (span.contains(&r.start) || r.start == span.end)
                    {
                        let cursor = |n: usize| {
                            egui::text::CCursor::new(
                                raw[..n.saturating_sub(start).min(raw.len())]
                                    .replace("\\|", "|")
                                    .chars()
                                    .count(),
                            )
                        };
                        let mut state =
                            egui::TextEdit::load_state(ui.ctx(), id).unwrap_or_default();
                        state
                            .cursor
                            .set_char_range(Some(egui::text::CCursorRange::two(
                                cursor(r.start),
                                cursor(r.end),
                            )));
                        state.store(ui.ctx(), id);
                        ui.memory_mut(|m| m.request_focus(id));
                    }
                    let out = egui::TextEdit::singleline(&mut value)
                        .id(id)
                        .desired_width(width)
                        .layouter(&mut layout)
                        .show(ui);
                    if out.response.has_focus()
                        && let Some(cursor) = out.cursor_range
                    {
                        let r = cursor.as_sorted_char_range();
                        let byte = |n| {
                            start
                                + value[..crate::document::byte_at(&value, n)]
                                    .replace('|', "\\|")
                                    .len()
                        };
                        selection = Some(byte(r.start)..byte(r.end));
                    }
                    if out.response.changed() {
                        let mut value = value.replace('|', "\\|");
                        // The parser synthesizes missing trailing cells at the next line.
                        if span.is_empty() {
                            let end = text[..span.start].trim_end_matches(['\r', '\n']).len();
                            let first = cells.iter().position(Range::is_empty).unwrap_or(col);
                            let prefix = if text[..end].trim_end().ends_with('|') {
                                ""
                            } else {
                                "|"
                            };
                            value = format!("{prefix}{} {value} |", " |".repeat(col - first));
                            selection = Some(end + value.len() - 2..end + value.len() - 2);
                            replacement = Some((end..end, value));
                        } else if raw.is_empty() {
                            replacement = Some((span.clone(), format!(" {value} ")));
                        } else {
                            replacement = Some((start..start + raw.len(), value));
                        }
                    }
                }
            });
        }
    });
    if ui.small_button("+ Row").clicked() {
        replacement = Some((table.end..table.end, format!("\n|{}", "   |".repeat(cols))));
    }
    if ui.small_button("+ Column").clicked() {
        let mut position = table.start;
        let mut ends = Vec::new();
        for line in text[table.start..table.end].split_inclusive('\n').take(2) {
            let trimmed = line.trim_end();
            ends.push((position + trimmed.len(), trimmed.ends_with('|')));
            position += line.len();
        }
        for (index, (end, trailing_pipe)) in ends.into_iter().enumerate().rev() {
            let prefix = if trailing_pipe { "" } else { " |" };
            let cell = if index == 0 { " Column |" } else { " --- |" };
            text.insert_str(end, &format!("{prefix}{cell}"));
        }
        return Some((true, Some(table.start..table.start)));
    }
    let changed = replacement.is_some();
    if let Some((range, value)) = replacement {
        text.replace_range(range, &value);
    }
    Some((changed, selection))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typing_in_a_focused_cell_updates_only_that_cell() {
        let ctx = egui::Context::default();
        let mut text = "| A | B |\n| :- | -: |\n| first | second |".to_owned();
        let n = text.find("first").unwrap();
        for (focus, events) in [
            (Some(n..n + 5), vec![]),
            (None, vec![egui::Event::Text("a|b".into())]),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        edit(ui, &mut text, focus.clone(), 17.).unwrap();
                    });
                },
            );
        }
        assert_eq!(text, "| A | B |\n| :- | -: |\n| a\\|b | second |");
    }

    #[test]
    fn cell_ranges_preserve_escaped_pipes_and_alignment() {
        let s = "| A | B |\n| :- | -: |\n| hé\\|llo | 2 |\n\n";
        let t = Table::parse(s).unwrap();
        assert_eq!(s[t.rows[1][0].clone()].trim(), "hé\\|llo");
        assert_eq!(&s[t.end..], "\n\n");
        assert!(Table::parse(&format!("{s}next")).is_none());
    }
}
