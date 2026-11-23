//! Edit a single GFM table while preserving the surrounding document.
use eframe::egui;
use pulldown_cmark::{Event, Parser, Tag};
use std::ops::Range;

const CELL_PADDING: [char; 2] = [' ', '\t'];

enum TableAction {
    RowBefore,
    RowAfter,
    RemoveRow,
    ColumnLeft,
    ColumnRight,
    RemoveColumn,
    Align(&'static str),
    PasteGrid(String),
    Remove,
}

fn pasted_cell_text(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    for part in value.split_inclusive('|') {
        if let Some(before_pipe) = part.strip_suffix('|') {
            output.push_str(before_pipe);
            let backslashes = before_pipe
                .bytes()
                .rev()
                .take_while(|&b| b == b'\\')
                .count();
            output.extend(std::iter::repeat_n('\\', backslashes));
            output.push('|');
        } else {
            output.push_str(part);
        }
    }
    output
}

// Spreadsheet clipboards quote cells containing tabs, newlines, or double quotes.
fn clipboard_rows(grid: &str) -> Vec<Vec<String>> {
    let plain = || {
        grid.lines()
            .map(|line| line.split('\t').map(str::to_owned).collect())
            .collect()
    };
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut input = grid;
    loop {
        let value = if let Some(quoted) = input.strip_prefix('"') {
            let mut field = String::new();
            let mut rest = quoted;
            loop {
                let Some((part, after)) = rest.split_once('"') else {
                    return plain();
                };
                field.push_str(part);
                if let Some(next) = after.strip_prefix('"') {
                    field.push('"');
                    rest = next;
                } else if after.is_empty() || after.starts_with(['\t', '\n']) {
                    input = after;
                    break field.replace('\n', " ");
                } else {
                    return plain();
                }
            }
        } else {
            let end = input.find(['\t', '\n']).unwrap_or(input.len());
            let (field, after) = input.split_at(end);
            input = after;
            field.to_owned()
        };
        row.push(value);
        if let Some(next) = input.strip_prefix('\t') {
            input = next;
        } else {
            rows.push(std::mem::take(&mut row));
            input = input.strip_prefix('\n').unwrap_or_default();
            if input.is_empty() {
                return rows;
            }
        }
    }
}

struct Table {
    alignment: Vec<pulldown_cmark::Alignment>,
    rows: Vec<Vec<Range<usize>>>,
    end: usize,
    start: usize,
}

impl Table {
    fn change(&self, text: &mut String, row: usize, col: usize, action: TableAction) {
        if matches!(action, TableAction::Remove) {
            text.replace_range(self.start..self.end, "");
            return;
        }
        // Split only on actual pipe delimiters. Keep escaped pipes and spare
        // cells in ragged source rows when changing the table structure.
        let mut rows: Vec<Vec<String>> = text[self.start..self.end]
            .lines()
            .enumerate()
            .map(|(row, line)| {
                let mut cells = Vec::new();
                let line = line.trim_matches(CELL_PADDING);
                let mut start = 0;
                let mut escaped = false;
                for (i, c) in line.char_indices() {
                    if c == '|' && !escaped {
                        cells.push(line[start..i].trim_matches(CELL_PADDING).to_owned());
                        start = i + 1;
                    }
                    // pulldown-cmark accepts an even run before a pipe in body cells.
                    escaped = c == '\\' && (row >= 2 || !escaped);
                }
                cells.push(line[start..].trim_matches(CELL_PADDING).to_owned());
                if line.starts_with('|') {
                    cells.remove(0);
                }
                if start == line.len() && line.ends_with('|') {
                    cells.pop();
                }
                cells
            })
            .collect();
        let cols = self.rows[0].len();
        let index = if row == 0 { 0 } else { row + 1 };
        match action {
            TableAction::RowBefore | TableAction::RowAfter => {
                let at = if row == 0 {
                    2
                } else {
                    index + usize::from(matches!(action, TableAction::RowAfter))
                };
                rows.insert(at, vec![String::new(); cols]);
            }
            TableAction::RemoveRow if row > 0 => {
                rows.remove(index);
            }
            TableAction::ColumnLeft | TableAction::ColumnRight => {
                let at = col + usize::from(matches!(action, TableAction::ColumnRight));
                for (i, line) in rows.iter_mut().enumerate() {
                    if line.len() < at {
                        line.resize(at, String::new());
                    }
                    line.insert(
                        at,
                        match i {
                            0 => "Column",
                            1 => "---",
                            _ => "",
                        }
                        .into(),
                    );
                }
            }
            TableAction::RemoveColumn if cols > 1 => {
                for line in &mut rows {
                    if col < line.len() {
                        line.remove(col);
                    }
                }
            }
            TableAction::PasteGrid(grid) => {
                let grid = grid.replace("\r\n", "\n").replace('\r', "\n");
                let grid = clipboard_rows(&grid);
                let width = grid
                    .iter()
                    .map(|line| col + line.len())
                    .max()
                    .unwrap_or(cols)
                    .max(cols);
                rows[0].resize_with(width, String::new);
                rows[1].resize_with(width, || "---".into());
                for (offset, line) in grid.into_iter().enumerate() {
                    let target = row + offset;
                    let index = target + usize::from(target > 0);
                    if index == rows.len() {
                        rows.push(Vec::new());
                    }
                    let cells = &mut rows[index];
                    let end = col + line.len();
                    if cells.len() < end {
                        cells.resize_with(end, String::new);
                    }
                    for (c, value) in line.into_iter().enumerate() {
                        cells[col + c] = value.replace('\\', "\\\\").replace('|', "\\|");
                    }
                }
            }
            TableAction::Align(marker) => rows[1][col] = marker.into(),
            _ => return,
        }
        let output = rows
            .into_iter()
            .map(|cells| format!("| {} |", cells.join(" | ")))
            .collect::<Vec<_>>()
            .join("\n");
        text.replace_range(self.start..self.end, &output);
    }
    fn parse(text: &str) -> Option<Self> {
        let mut events = Parser::new_ext(text, crate::blocks::options()).into_offset_iter();
        let (Event::Start(Tag::Table(alignment)), span) = events.next()? else {
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
            alignment,
            rows,
            start: span.start,
            end: text[..span.end].trim_end_matches(['\r', '\n']).len(),
        })
    }
}

/// A single grid for read mode keeps cell widths stable when entering a table.
fn cell_menu(response: &egui::Response, row: usize, cols: usize) -> Option<TableAction> {
    let mut chosen = None;
    response.context_menu(|ui| {
        for (label, action, enabled) in [
            ("Insert row above", TableAction::RowBefore, row > 0),
            ("Insert row below", TableAction::RowAfter, true),
            ("Delete row", TableAction::RemoveRow, row > 0),
            ("Insert column left", TableAction::ColumnLeft, true),
            ("Insert column right", TableAction::ColumnRight, true),
            ("Delete column", TableAction::RemoveColumn, cols > 1),
            ("Align left", TableAction::Align(":---"), true),
            ("Align center", TableAction::Align(":---:"), true),
            ("Align right", TableAction::Align("---:"), true),
            ("Remove table", TableAction::Remove, true),
        ] {
            if matches!(
                action,
                TableAction::ColumnLeft | TableAction::Align(":---") | TableAction::Remove
            ) {
                ui.separator();
            }
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                chosen = Some(action);
                ui.close_menu();
            }
        }
    });
    chosen
}

fn cell_source_byte(raw: &str, cursor: usize) -> usize {
    let mut chars = raw.char_indices().peekable();
    for _ in 0..cursor {
        let Some((_, c)) = chars.next() else { break };
        if c == '\\' && chars.peek().is_some_and(|(_, next)| *next == '|') {
            chars.next();
        }
    }
    chars.next().map_or(raw.len(), |(byte, _)| byte)
}

pub fn preview(
    ui: &mut egui::Ui,
    text: &str,
    size: f32,
    offset: usize,
) -> Option<crate::window::BlockAction> {
    let table = Table::parse(text)?;
    // Delegate embedded media and links to the full CommonMark viewer.
    if text.contains('[') || text.contains('<') {
        return None;
    }
    let cols = table.rows.first()?.len().max(1);
    let width = ((ui.available_width() - 1.) / cols as f32).max(64.);
    let mut hit = crate::window::BlockAction::None;
    egui::ScrollArea::horizontal().show(ui, |ui| {
        ui.spacing_mut().item_spacing = egui::Vec2::ZERO;
        for (row, cells) in table.rows.iter().enumerate() {
            let jobs: Vec<_> = cells
                .iter()
                .map(|span| {
                    let raw = text[span.clone()].trim_matches(CELL_PADDING);
                    let decoded;
                    let display = if raw.contains("\\|") {
                        decoded = raw.replace("\\|", "|");
                        &decoded
                    } else {
                        raw
                    };
                    let mut job = crate::window::layout(
                        display,
                        width - 20.,
                        ui.visuals().text_color(),
                        false,
                        0,
                        size * 0.93,
                    );
                    if row == 0 {
                        for s in &mut job.sections {
                            s.format.font_id.family = egui::FontFamily::Name("Bold".into());
                        }
                    }
                    ui.fonts(|f| f.layout_job(job))
                })
                .collect();
            let height = jobs.iter().map(|g| g.size().y).fold(size, f32::max) + 14.;
            ui.horizontal(|ui| {
                for (col, (span, galley)) in cells.iter().zip(jobs).enumerate() {
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
                    if let Some(action) = cell_menu(&response, row, cols) {
                        let mut updated = text.to_owned();
                        table.change(&mut updated, row, col, action);
                        hit = crate::window::BlockAction::ReplaceTable(
                            offset..offset + text.len(),
                            updated,
                        );
                    }
                    let header = if ui.visuals().dark_mode {
                        egui::Color32::from_rgb(47, 57, 68)
                    } else {
                        egui::Color32::from_rgb(231, 237, 241)
                    };
                    ui.painter().rect_filled(
                        rect,
                        0.,
                        if row == 0 {
                            header
                        } else {
                            ui.visuals().panel_fill
                        },
                    );
                    ui.painter().rect_stroke(
                        rect,
                        0.,
                        ui.visuals().widgets.noninteractive.bg_stroke,
                        egui::StrokeKind::Inside,
                    );
                    let inset = match table.alignment[col] {
                        pulldown_cmark::Alignment::Right => width - galley.size().x - 10.,
                        pulldown_cmark::Alignment::Center => (width - galley.size().x) * 0.5,
                        _ => 10.,
                    };
                    let origin = rect.min + egui::vec2(inset.max(4.), 7.);
                    if response.clicked()
                        && let Some(pos) = response.interact_pointer_pos()
                    {
                        let raw = &text[span.clone()];
                        let start =
                            span.start + raw.len() - raw.trim_start_matches(CELL_PADDING).len();
                        hit = crate::window::BlockAction::Edit(
                            offset
                                + start
                                + cell_source_byte(
                                    raw.trim_matches(CELL_PADDING),
                                    galley.cursor_from_pos(pos - origin).ccursor.index,
                                ),
                        );
                    }
                    ui.painter()
                        .galley(origin, galley, ui.visuals().text_color());
                }
            });
        }
    });
    Some(hit)
}

pub fn edit(
    ui: &mut egui::Ui,
    text: &mut String,
    focus: Option<Range<usize>>,
    size: f32,
) -> Option<(bool, Option<Range<usize>>)> {
    let table = Table::parse(text)?;
    let focus = ui
        .ctx()
        .data_mut(|d| d.remove_temp::<usize>(ui.id().with("cell focus")))
        .map(|at| at..at)
        .or(focus);
    let mut replacement = None;
    let mut structural = None;
    let mut next_cell = None;
    let mut selection = None;
    let cols = table.rows.first()?.len().max(1);
    let width = ((ui.available_width() - 8. * cols as f32) / cols as f32).max(45.);
    egui::ScrollArea::horizontal().show(ui, |ui| {
        for (row, cells) in table.rows.iter().enumerate() {
            ui.horizontal(|ui| {
                for (col, span) in cells.iter().enumerate() {
                    let original = &text[span.clone()];
                    let raw = original.trim_matches(CELL_PADDING);
                    let start = if raw.is_empty() && !span.is_empty() {
                        span.start + 1
                    } else {
                        span.start + original.len()
                            - original.trim_start_matches(CELL_PADDING).len()
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
                    let focused = ui.memory(|m| m.has_focus(id));
                    if focused {
                        ui.input_mut(|input| {
                            if let Some(index) = input.events.iter().position(
                                |e| matches!(e, egui::Event::Paste(s) if s.contains('\t')),
                            ) && let egui::Event::Paste(grid) = input.events.remove(index)
                            {
                                structural = Some((row, col, TableAction::PasteGrid(grid)));
                            }
                            for event in &mut input.events {
                                if let egui::Event::Paste(text) = event {
                                    // GFM table cells occupy one source line.
                                    *text = pasted_cell_text(text)
                                        .replace("\r\n", " ")
                                        .replace(['\r', '\n'], " ");
                                }
                            }
                        });
                    }
                    if focused
                        && (row > 0 || col > 0)
                        && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Tab))
                    {
                        next_cell = Some(if col > 0 {
                            (row, col - 1)
                        } else {
                            (row - 1, cols - 1)
                        });
                    } else if focused
                        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab))
                    {
                        if col + 1 < cols {
                            next_cell = Some((row, col + 1));
                        } else {
                            next_cell = Some((row + 1, 0));
                            if row + 1 == table.rows.len() {
                                structural = Some((row, col, TableAction::RowAfter));
                            }
                        }
                    } else if focused
                        && ui.input_mut(|i| i.consume_key(egui::Modifiers::SHIFT, egui::Key::Enter))
                    {
                        next_cell = Some((row.saturating_sub(1), col));
                    } else if focused
                        && ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
                    {
                        next_cell = Some((row + 1, col));
                        if row + 1 == table.rows.len() {
                            structural = Some((row, col, TableAction::RowAfter));
                        }
                    }
                    let fill = if row == 0 {
                        ui.visuals().faint_bg_color
                    } else {
                        ui.visuals().panel_fill
                    };
                    let border = ui.visuals().widgets.noninteractive.bg_stroke;
                    let out = egui::Frame::new()
                        .fill(fill)
                        .stroke(border)
                        .inner_margin(egui::Margin::symmetric(6, 4))
                        .show(ui, |ui| {
                            egui::TextEdit::singleline(&mut value)
                                .id(id)
                                .frame(false)
                                .desired_width((width - 14.).max(35.))
                                .layouter(&mut layout)
                                .show(ui)
                        })
                        .inner;
                    if let Some(action) = cell_menu(&out.response, row, cols) {
                        structural = Some((row, col, action));
                    }
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
                            let prefix =
                                if text[..end].trim_end_matches(CELL_PADDING).ends_with('|') {
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
    ui.horizontal(|ui| {
        if ui.small_button("+ Row").clicked() {
            structural = Some((table.rows.len() - 1, 0, TableAction::RowAfter));
        }
        if ui.small_button("+ Column").clicked() {
            structural = Some((0, cols - 1, TableAction::ColumnRight));
        }
    });
    if let Some((row, col)) = next_cell {
        if let Some((r, c, action)) = structural.take() {
            table.change(text, r, c, action);
        }
        let updated = Table::parse(text)?;
        let span = updated.rows.get(row)?.get(col)?;
        let at = span.start + text[span.clone()].len()
            - text[span.clone()].trim_start_matches(CELL_PADDING).len();
        ui.ctx()
            .data_mut(|d| d.insert_temp(ui.id().with("cell focus"), at));
        ui.ctx().request_repaint();
        return Some((row >= table.rows.len(), Some(at..at)));
    }
    if let Some((row, col, action)) = structural {
        table.change(text, row, col, action);
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
    fn rendered_inline_code_decodes_table_pipes_and_keeps_source_positions() {
        let ctx = egui::Context::default();
        crate::configure_fonts(&ctx);
        let source = "| Name |\n| --- |\n| `é\\|b` |";
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| preview(ui, source, 17., 0));
        });
        assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().replace('\u{200b}', "") == "é|b")));
        let raw = r"`é\|b`";
        assert_eq!(&raw[cell_source_byte(raw, 3)..], "b`");
    }

    #[test]
    fn right_click_rendered_cell_can_insert_a_row() {
        let ctx = egui::Context::default();
        crate::configure_fonts(&ctx);
        let source = "| Name |\n| --- |\n| Cedar |";
        let mut point = egui::Pos2::ZERO;
        let mut changed = None;
        for frame in 0..7 {
            let mut events = vec![];
            if matches!(frame, 1 | 2 | 5 | 6) {
                let button = if frame < 3 {
                    egui::PointerButton::Secondary
                } else {
                    egui::PointerButton::Primary
                };
                events = vec![
                    egui::Event::PointerMoved(point),
                    egui::Event::PointerButton {
                        pos: point,
                        button,
                        pressed: matches!(frame, 1 | 5),
                        modifiers: Default::default(),
                    },
                ];
            }
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800., 600.),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if let Some(crate::window::BlockAction::ReplaceTable(_, md)) =
                            preview(ui, source, 17., 0)
                        {
                            changed = Some(md);
                        }
                    });
                },
            );
            if frame == 0 || frame == 4 {
                let label = if frame == 0 {
                    "Cedar"
                } else {
                    "Insert row below"
                };
                point = out
                    .shapes
                    .iter()
                    .find_map(|s| match &s.shape {
                        egui::Shape::Text(t) if t.galley.text() == label => {
                            Some(t.pos + t.galley.size() * 0.5)
                        }
                        _ => None,
                    })
                    .expect(label);
            }
        }
        assert_eq!(
            changed.as_deref(),
            Some("| Name |\n| --- |\n| Cedar |\n|  |")
        );
    }

    #[test]
    fn contextual_table_edits_preserve_escaped_content() {
        let mut md = "| A | B |\n| --- | ---: |\n| a\\|b | 2 |\n\n".to_owned();
        for (row, col, action) in [
            (1, 1, TableAction::RowBefore),
            (1, 0, TableAction::ColumnLeft),
            (1, 2, TableAction::Align(":---:")),
            (1, 0, TableAction::RemoveRow),
            (1, 0, TableAction::RemoveColumn),
        ] {
            Table::parse(&md).unwrap().change(&mut md, row, col, action);
        }
        assert_eq!(md, "| A | B |\n| --- | :---: |\n| a\\|b | 2 |\n\n");
    }
    #[test]
    fn adding_a_row_preserves_typographic_spaces_in_cells() {
        let mut md = "| A | B |\n| --- | --- |\n|\u{a0}|\u{2003}Paris\u{202f}|".to_owned();
        Table::parse(&md)
            .unwrap()
            .change(&mut md, 1, 0, TableAction::RowAfter);
        assert!(md.contains("| \u{a0} | \u{2003}Paris\u{202f} |"));
    }

    #[test]
    fn tab_from_last_cell_creates_row_and_keeps_typing_in_table() {
        let ctx = egui::Context::default();
        let mut text = "| A | B |\n| --- | --- |\n| first | last |".to_owned();
        let n = text.find("last").unwrap();
        let tab = egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        };
        for (focus, events) in [
            (Some(n..n), vec![]),
            (None, vec![tab]),
            (None, vec![egui::Event::Text("next".into())]),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| edit(ui, &mut text, focus.clone(), 17.));
                },
            );
        }
        assert!(text.ends_with("| next |  |"), "{text}");
    }
    #[test]
    fn shift_tab_moves_from_first_body_cell_to_last_header() {
        let ctx = egui::Context::default();
        let mut text = "| A | B |\n| --- | --- |\n| first | last |".to_owned();
        let n = text.find("first").unwrap();
        let tab = egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::SHIFT,
        };
        for (focus, events) in [
            (Some(n..n), vec![]),
            (None, vec![tab]),
            (None, vec![egui::Event::Text("next".into())]),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| edit(ui, &mut text, focus.clone(), 17.));
                },
            );
        }
        assert_eq!(text, "| A | nextB |\n| --- | --- |\n| first | last |");
    }
    #[test]
    fn enter_adds_a_row_and_shift_enter_moves_back_in_the_same_column() {
        let ctx = egui::Context::default();
        let mut text = "| A | B |\n| --- | --- |\n| one | two |".to_owned();
        let start = text.find("two").unwrap();
        let enter = |modifiers| egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        for (focus, events) in [
            (Some(start..start), vec![]),
            (None, vec![enter(egui::Modifiers::NONE)]),
            (None, vec![egui::Event::Text("three".into())]),
            (None, vec![enter(egui::Modifiers::SHIFT)]),
            (None, vec![egui::Event::Text("before ".into())]),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| edit(ui, &mut text, focus.clone(), 17.));
                },
            );
        }
        let table = Table::parse(&text).unwrap();
        assert_eq!(table.rows.len(), 3);
        assert_eq!(text[table.rows[2][1].clone()].trim(), "three");
        assert_eq!(text[table.rows[1][1].clone()].trim(), "before two");
    }

    #[test]
    fn quoted_clipboard_cells_preserve_boundaries() {
        assert_eq!(
            clipboard_rows("\"one\ttwo\"\t\"three\nfour\"\n\"a\"\"b\"\t\n"),
            [vec!["one\ttwo", "three four"], vec!["a\"b", ""]]
        );
        assert_eq!(
            clipboard_rows("\"unfinished\tvalue"),
            [vec!["\"unfinished", "value"]]
        );
    }

    #[test]
    fn spreadsheet_paste_expands_table_from_focused_cell() {
        let ctx = egui::Context::default();
        crate::configure_fonts(&ctx);
        let mut text = "| A | B |\n| :--- | ---: |\n| keep | here |".to_owned();
        let start = text.find("here").unwrap();
        for (focus, events) in [
            (Some(start..start), vec![]),
            (
                None,
                vec![egui::Event::Paste(
                    "\"α\tβ\"\tb|c\r\n🦀\t\"d\r\ne\"\r\n".into(),
                )],
            ),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| edit(ui, &mut text, focus.clone(), 17.));
                },
            );
        }
        assert_eq!(
            text,
            "| A | B |  |\n| :--- | ---: | --- |\n| keep | α\tβ | b\\|c |\n|  | 🦀 | d e |"
        );
        assert_eq!(Table::parse(&text).unwrap().rows.len(), 3);
    }

    #[test]
    fn adding_a_row_keeps_even_escaped_pipes_in_existing_body_cells() {
        let source = "| A | B |\n| --- | --- |\n| a\\\\|b | second |";
        let mut text = source.to_owned();
        let table = Table::parse(&text).unwrap();
        assert_eq!(text[table.rows[1][1].clone()].trim(), "second");
        table.change(&mut text, 1, 0, TableAction::RowAfter);
        assert_eq!(text, format!("{source}\n|  |  |"));
    }

    #[test]
    fn pasted_backslash_and_pipe_stay_in_one_cell() {
        let ctx = egui::Context::default();
        let mut text = "| A | B |\n| --- | --- |\n| first | second |".to_owned();
        let n = text.find("first").unwrap();
        for (focus, events) in [
            (Some(n..n + 5), vec![]),
            (None, vec![egui::Event::Paste(r"a\|b".into())]),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .show(ctx, |ui| edit(ui, &mut text, focus.clone(), 17.));
                },
            );
        }
        let table = Table::parse(&text).unwrap();
        assert_eq!(text[table.rows[1][1].clone()].trim(), "second");
        let mut html = String::new();
        pulldown_cmark::html::push_html(
            &mut html,
            Parser::new_ext(&text, crate::blocks::options()),
        );
        assert!(html.contains(r"<td>a\|b</td>"), "{html}");
    }

    #[test]
    fn visual_cell_edit_round_trips_through_a_markdown_file() {
        let ctx = egui::Context::default();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("table.md");
        let original = "\u{feff}| A | B |\r\n| :- | -: |\r\n| first | second |\r\n";
        std::fs::write(&path, original).unwrap();
        let mut doc = crate::document::Document::open(&path).unwrap();
        let n = doc.text.find("first").unwrap();
        for (focus, events) in [
            (Some(n..n + 5), vec![]),
            (
                None,
                vec![egui::Event::Paste("é|🦀\r\nnext\u{202f}".into())],
            ),
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        edit(ui, &mut doc.text, focus.clone(), 17.).unwrap();
                    });
                },
            );
        }
        doc.save(&path).unwrap();
        let expected = original.replace("first", "é\\|🦀 next\u{202f}");
        assert_eq!(std::fs::read(&path).unwrap(), expected.as_bytes());
        let reopened = crate::document::Document::open(&path).unwrap();
        let table = Table::parse(&reopened.text).unwrap();
        assert_eq!(table.rows[1].len(), 2);
        assert_eq!(
            &reopened.text[table.rows[1][0].clone()],
            " é\\|🦀 next\u{202f} "
        );
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
