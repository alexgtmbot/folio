use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};

// egui's file loader reads a filesystem path verbatim. Resolve URI escapes only
// for display, keeping the portable Markdown destination intact on disk.
fn local_images<'a>(text: &'a str, base: Option<&std::path::Path>) -> std::borrow::Cow<'a, str> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    if !text.contains("![") {
        return text.into();
    }
    let mut events = Parser::new_ext(text, crate::blocks::options()).into_offset_iter();
    let mut display = String::new();
    let mut previous = 0;
    while let Some((event, span)) = events.next() {
        let Event::Start(Tag::Image {
            dest_url, title, ..
        }) = event
        else {
            continue;
        };
        let raw = dest_url.strip_prefix("file://").unwrap_or(&dest_url);
        if raw.contains("://") || raw.starts_with("data:") {
            continue;
        }
        let Ok(decoded) = percent_encoding::percent_decode_str(raw).decode_utf8() else {
            continue;
        };
        if decoded.chars().any(char::is_control) {
            continue;
        }
        if decoded == raw && !std::path::Path::new(raw).is_absolute() {
            continue;
        }
        let path = base
            .unwrap_or(std::path::Path::new(""))
            .join(decoded.as_ref());
        let mut depth = 1;
        display.push_str(&text[previous..span.start]);
        display.push_str("![");
        for (event, _) in events.by_ref() {
            match event {
                Event::Start(Tag::Image { .. }) => depth += 1,
                Event::End(TagEnd::Image) => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                Event::Text(s) | Event::Code(s) => {
                    for c in s.chars() {
                        if c.is_ascii_punctuation() {
                            display.push('\\');
                        }
                        display.push(c);
                    }
                }
                _ => {}
            }
        }
        display.push_str("](<file://");
        for c in path.to_string_lossy().chars() {
            if matches!(c, '\\' | '<' | '>' | '&' | '|') {
                display.push('\\');
            }
            display.push(c);
        }
        display.push('>');
        if !title.is_empty() {
            display.push_str(" \"");
            for c in title.chars() {
                if matches!(c, '"' | '\\' | '&') {
                    display.push('\\');
                }
                display.push(c);
            }
            display.push('"');
        }
        display.push(')');
        previous = span.end;
    }
    if previous == 0 {
        return text.into();
    }
    display.push_str(&text[previous..]);
    display.into()
}

// CommonMark permits displaying soft breaks as line breaks. Keep the editor's line layout in preview.
fn preview_text(text: &str) -> std::borrow::Cow<'_, str> {
    use pulldown_cmark::{Event, Parser};
    let mut out = String::new();
    let mut start = 0;
    for (event, r) in Parser::new_ext(text, crate::blocks::options()).into_offset_iter() {
        if event == Event::SoftBreak {
            out.push_str(&text[start..r.start]);
            out.push_str("  ");
            start = r.start;
        }
    }
    if out.is_empty() {
        return text.into();
    }
    out.push_str(&text[start..]);
    out.into()
}

pub fn preview(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    text: &str,
    base: Option<&std::path::Path>,
    size: f32,
) {
    render(ui, cache, text, base, size, false);
}

fn render(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    text: &str,
    base: Option<&std::path::Path>,
    size: f32,
    interactive: bool,
) -> Option<(usize, bool)> {
    let mut viewer = egui_commonmark::CommonMarkViewer::new()
        .max_image_width(Some(ui.available_width().max(1.) as usize));
    if let Some(base) = base {
        viewer = viewer.default_implicit_uri_scheme(format!("file://{}/", base.display()));
    }
    ui.scope(|ui| {
        let style = ui.style_mut();
        style
            .text_styles
            .insert(egui::TextStyle::Body, FontId::proportional(size));
        style.text_styles.insert(
            egui::TextStyle::Heading,
            FontId::new(size * 1.9, egui::FontFamily::Name("Bold".into())),
        );
        style
            .text_styles
            .insert(egui::TextStyle::Monospace, FontId::monospace(size * 0.9));
        style.spacing.item_spacing.y = 6.;
        ui.add_space(8.);
        let local = local_images(text, base);
        let display = preview_text(&local);
        if interactive && text.contains('[') {
            let mut edited = display.to_string();
            viewer.show_mut(ui, cache, &mut edited);
            if edited != display {
                let difference = edited
                    .bytes()
                    .zip(display.bytes())
                    .position(|(a, b)| a != b)?;
                return tasks(&display)
                    .position(|r| r.contains(&difference))
                    .map(|index| (index, edited.as_bytes()[difference] != b' '));
            }
        } else {
            viewer.show(ui, cache, &display);
        }
        None
    })
    .inner
}

fn tasks(text: &str) -> impl Iterator<Item = std::ops::Range<usize>> + '_ {
    pulldown_cmark::Parser::new_ext(text, crate::blocks::options())
        .into_offset_iter()
        .filter_map(|(e, r)| matches!(e, pulldown_cmark::Event::TaskListMarker(_)).then_some(r))
}
pub enum BlockAction {
    Edit(usize),
    Task(usize, bool),
    None,
}
/// Match painted glyph runs to parser text in reading order. This also handles
/// multiple equally named links and repeated words in separate table cells.
fn clicked_byte(ui: &egui::Ui, first: usize, pointer: egui::Pos2, source: &str) -> Option<usize> {
    use pulldown_cmark::{Event, Parser};
    let mut visible = String::new();
    let mut positions = Vec::new();
    for (event, range) in Parser::new_ext(source, crate::blocks::options()).into_offset_iter() {
        let value = match event {
            Event::Text(t) | Event::Code(t) => t,
            _ => continue,
        };
        let raw = &source[range.clone()];
        let start = raw.find(value.as_ref()).map(|n| range.start + n);
        for (i, c) in value.char_indices() {
            positions.push((
                visible.len(),
                start.map_or(range.start, |s| s + i),
                start.map_or(range.end, |s| s + i + c.len_utf8()),
            ));
            visible.push(c);
        }
    }
    ui.ctx().graphics(|graphics| {
        let list = graphics.get(ui.layer_id())?;
        let mut from = 0;
        let mut closest = None;
        for entry in list.all_entries().skip(first) {
            let egui::Shape::Text(shape) = &entry.shape else {
                continue;
            };
            let run = shape.galley.text();
            let run = run.trim_matches('\n');
            if run.is_empty() {
                continue;
            }
            let Some(relative) = visible[from..].find(run) else {
                continue;
            };
            let start = from + relative;
            from = start + run.len();
            let rect = shape
                .galley
                .rect
                .translate(shape.pos.to_vec2())
                .intersect(entry.clip_rect);
            let distance = rect.distance_sq_to_pos(pointer);
            if closest.is_some_and(|(d, _)| d <= distance) {
                continue;
            }
            let cursor = shape
                .galley
                .cursor_from_pos(pointer - shape.pos)
                .ccursor
                .index;
            let byte = start + crate::document::byte_at(run, cursor);
            let index = positions.partition_point(|(i, _, _)| *i < byte);
            let offset = if byte == from {
                positions[index - 1].2
            } else {
                positions[index].1
            };
            closest = Some((distance, offset));
        }
        closest.map(|(_, byte)| byte)
    })
}

pub fn editable_block(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    text: &str,
    r: &std::ops::Range<usize>,
    definitions: &[std::ops::Range<usize>],
    base: Option<&std::path::Path>,
    size: f32,
) -> BlockAction {
    let pointer = ui.input(|i| {
        i.pointer
            .primary_clicked()
            .then(|| i.pointer.interact_pos())
            .flatten()
    });
    let first = pointer.map(|_| {
        ui.ctx()
            .graphics(|g| g.get(ui.layer_id()).map_or(0, |list| list.next_idx().0))
    });
    let source = crate::blocks::with_references(text, r, definitions);
    let response = ui.push_id(r.start, |ui| {
        let task = render(ui, cache, &source, base, size, true);
        ui.add_space(5.);
        task
    });
    if let Some((index, checked)) = response.inner
        && let Some(marker) = tasks(&text[r.clone()]).nth(index)
    {
        return BlockAction::Task(r.start + marker.start + 1, checked);
    }
    let rect = response.response.rect;
    if let Some(p) = pointer
        && rect.contains(p)
        && ui.clip_rect().contains(p)
    {
        let block = &text[r.clone()];
        let byte = clicked_byte(ui, first.unwrap_or(0), p, block)
            .unwrap_or_else(|| folio::block_prefix(block));
        BlockAction::Edit(r.start + byte)
    } else {
        BlockAction::None
    }
}

pub fn outline(text: &str) -> Vec<(usize, String)> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let mut result = Vec::new();
    let mut heading = None;
    let mut last = 0;
    let mut chars = 0;
    for (event, range) in Parser::new_ext(text, crate::blocks::options()).into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { .. }) => {
                chars += text[last..range.start].chars().count();
                last = range.start;
                heading = Some((chars, String::new()));
            }
            Event::End(TagEnd::Heading(_)) => {
                if let Some(entry) = heading.take() {
                    result.push(entry);
                }
            }
            Event::Text(t) | Event::Code(t) => {
                if let Some((_, name)) = &mut heading {
                    name.push_str(&t);
                }
            }
            _ => {}
        }
    }
    result
}

pub fn layout(
    text: &str,
    width: f32,
    color: Color32,
    source: bool,
    _caret: usize,
    size: f32,
) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    let body = TextFormat {
        font_id: FontId::proportional(size),
        color,
        ..Default::default()
    };
    let mono = TextFormat {
        font_id: FontId::monospace(size * 0.9),
        ..body.clone()
    };
    if source {
        job.append(text, 0., mono);
        return job;
    }
    use pulldown_cmark::{Event, Parser, Tag};
    let mut faces = vec![body.clone()];
    let mut pos = 0;
    // egui indexes the galley and the source by Unicode scalar. Keep one
    // invisible glyph per delimiter so selections still edit the original bytes.
    let append_gap = |job: &mut LayoutJob, gap: std::ops::Range<usize>| {
        let mut at = gap.start;
        for line in text[gap].split_inclusive('\n') {
            let trimmed = line.trim_start_matches([' ', '\t']);
            let indent = line.len() - trimmed.len();
            let line_start = at == 0 || text[..at].ends_with('\n');
            let bullet = line_start && ["- ", "+ ", "* "].iter().any(|p| trimmed.starts_with(p));
            let mut shown = String::new();
            for (i, c) in line.char_indices() {
                shown.push(if c == '\n' || (line_start && i < indent) {
                    c
                } else if bullet && i == indent {
                    '•'
                } else if bullet && i == indent + 1 {
                    ' '
                } else if line_start && (c.is_ascii_digit() || matches!(c, '.' | ')' | '>')) {
                    c
                } else {
                    '\u{200b}'
                });
            }
            job.append(&shown, 0., body.clone());
            at += line.len();
        }
    };
    for (event, r) in Parser::new_ext(text, crate::blocks::options()).into_offset_iter() {
        match event {
            Event::Start(tag) => {
                let mut f = faces.last().unwrap().clone();
                match tag {
                    Tag::Heading { level, .. } => {
                        f.font_id = FontId::new(
                            size * (2.0 - (level as usize - 1) as f32 * 0.17),
                            egui::FontFamily::Name("Bold".into()),
                        )
                    }
                    Tag::Strong => f.font_id.family = egui::FontFamily::Name("Bold".into()),
                    Tag::Emphasis => f.italics = true,
                    Tag::Strikethrough => f.strikethrough = egui::Stroke::new(1.0_f32, color),
                    Tag::CodeBlock(_) => f = mono.clone(),
                    Tag::Link { .. } => f.underline = egui::Stroke::new(1.0_f32, color),
                    _ => {}
                }
                faces.push(f);
            }
            Event::End(_) => {
                faces.pop();
            }
            _ if r.start >= pos => {
                append_gap(&mut job, pos..r.start);
                let f = if matches!(event, Event::Code(_)) {
                    &mono
                } else {
                    faces.last().unwrap()
                };
                match event {
                    Event::TaskListMarker(done) => {
                        let mark = if done { "☑" } else { "☐" };
                        job.append(
                            &format!(
                                "{mark}{}",
                                "\u{200b}"
                                    .repeat(text[r.clone()].chars().count().saturating_sub(1))
                            ),
                            0.,
                            f.clone(),
                        );
                    }
                    Event::Code(ref value) => {
                        let raw = &text[r.clone()];
                        let ticks = raw.bytes().take_while(|b| *b == b'`').count();
                        let trailing = raw.chars().count() - ticks - value.chars().count();
                        job.append(
                            &format!(
                                "{}{}{}",
                                "\u{200b}".repeat(ticks),
                                value,
                                "\u{200b}".repeat(trailing)
                            ),
                            0.,
                            f.clone(),
                        );
                    }
                    Event::Text(ref value)
                        if value.as_ref() != &text[r.clone()]
                            && value.chars().count() <= text[r.clone()].chars().count() =>
                    {
                        let padding = text[r.clone()].chars().count() - value.chars().count();
                        job.append(value, 0., f.clone());
                        job.append(&"\u{200b}".repeat(padding), 0., f.clone());
                    }
                    _ => job.append(&text[r.clone()], 0., f.clone()),
                }
                pos = r.end;
            }
            _ => {}
        }
    }
    append_gap(&mut job, pos..text.len());
    job
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn local_image_uri_is_decoded_only_for_display() {
        use pulldown_cmark::{Event, Parser, Tag};
        let md = "![the **map**][p]\n\n[p]: photos/Trail%20map.png";
        let display = local_images(md, Some(std::path::Path::new("notebook")));
        let dest = Parser::new(&display)
            .find_map(|e| match e {
                Event::Start(Tag::Image { dest_url, .. }) => Some(dest_url),
                _ => None,
            })
            .unwrap();
        assert_eq!(
            std::path::Path::new(dest.strip_prefix("file://").unwrap()),
            std::path::Path::new("notebook/photos/Trail map.png")
        );
        assert_eq!(
            local_images("![x](https://example.test/a%20b)", None),
            "![x](https://example.test/a%20b)"
        );
    }

    #[test]
    fn commonmark_spec_examples() {
        let examples: Vec<serde_json::Value> =
            serde_json::from_str(include_str!("../tests/fixtures/commonmark-0.31.2.json")).unwrap();
        assert_eq!(examples.len(), 652);
        let ctx = egui::Context::default();
        crate::configure_fonts(&ctx);
        let mut cache = egui_commonmark::CommonMarkCache::default();
        let mut differences = Vec::new();
        for e in examples {
            let md = e["markdown"].as_str().unwrap();
            let mut actual = String::new();
            pulldown_cmark::html::push_html(
                &mut actual,
                pulldown_cmark::Parser::new_ext(md, crate::blocks::options()),
            );
            let normalize = |s: &str| {
                s.replace("&quot;", "\"")
                    .replace(">\n<", "><")
                    .replace("<hr>", "<hr />")
                    .replace("<br>", "<br />")
            };
            if normalize(&actual) != normalize(e["html"].as_str().unwrap()) {
                differences.push(e["example"].as_u64().unwrap());
            }
            assert_eq!(
                layout(md, 500., Color32::WHITE, false, 0, 17.)
                    .text
                    .chars()
                    .count(),
                md.chars().count()
            );
            let ranges = crate::blocks::ranges(md);
            assert_eq!(
                ranges.iter().map(|r| &md[r.clone()]).collect::<String>(),
                md
            );
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default()
                    .show(ctx, |ui| preview(ui, &mut cache, md, None, 17.));
            });
        }
        assert!(
            differences.is_empty(),
            "HTML differences in examples {differences:?}"
        );
    }
    #[test]
    fn list_continuations_keep_their_lines() {
        let source = "- Tell me about your day\ntest\ntest\n- next\n\n```\na\nb\n```";
        assert_eq!(
            preview_text(source),
            "- Tell me about your day  \ntest  \ntest\n- next\n\n```\na\nb\n```"
        );
    }
    #[test]
    fn click_places_caret_inside_second_occurrence_of_formatted_unicode_text() {
        let source = "café **café** tail";
        let ctx = egui::Context::default();
        crate::configure_fonts(&ctx);
        let mut cache = Default::default();
        let mut click = None;
        let mut result = None;
        for frame in 0..3 {
            let mut input = egui::RawInput::default();
            if let Some(pos) = click
                && frame > 0
            {
                input.events = vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: frame == 1,
                        modifiers: Default::default(),
                    },
                ];
            }
            let output = ctx.run(input, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    if let BlockAction::Edit(byte) =
                        editable_block(ui, &mut cache, source, &(0..source.len()), &[], None, 17.)
                    {
                        result = Some(byte);
                    }
                });
            });
            if frame == 0 {
                for entry in output.shapes {
                    if let egui::Shape::Text(shape) = entry.shape
                        && let Some(start) = shape.galley.text().rfind("café")
                    {
                        let n = shape.galley.text()[..start].chars().count() + 2;
                        let cursor = shape.galley.from_ccursor(egui::text::CCursor::new(n));
                        click = Some(
                            shape.pos + shape.galley.pos_from_cursor(&cursor).center().to_vec2(),
                        );
                    }
                }
                assert!(click.is_some());
            }
        }
        assert_eq!(result, Some(source.rfind("café").unwrap() + 2));
    }
    #[test]
    fn clicking_rendered_task_after_soft_break_targets_original_byte() {
        let source = "# Intro\n\n- é [literal]\n  wrapped\n- [X] walk\n";
        let block = crate::blocks::ranges(source).into_iter().last().unwrap();
        let ctx = egui::Context::default();
        let mut cache = Default::default();
        let mut toggled = None;
        let mut frame = |events| {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        if let BlockAction::Task(byte, value) =
                            editable_block(ui, &mut cache, source, &block, &[], None, 17.)
                        {
                            toggled = Some((byte, value));
                        }
                    });
                },
            );
        };
        frame(vec![]);
        // Exercise the rendered hit target across font and platform metrics.
        for y in (35..130).step_by(6) {
            for x in (12..68).step_by(6) {
                let pos = egui::pos2(x as f32, y as f32);
                for pressed in [true, false] {
                    frame(vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ]);
                }
            }
        }
        assert_eq!(toggled, Some((source.find("[X]").unwrap() + 1, false)));
    }

    #[test]
    fn visual_text_decodes_entities_and_normalizes_code_spans() {
        let source = "Caf&eacute; &amp; ` a  b `";
        let visual = layout(source, 500., Color32::WHITE, false, 0, 17.).text;
        assert_eq!(visual.replace('\u{200b}', ""), "Café & a  b");
        assert_eq!(visual.chars().count(), source.chars().count());
    }
    #[test]
    fn typed_heading_and_emphasis_hide_delimiters_in_write() {
        let source = "# My **notes**";
        let job = layout(source, 500., Color32::WHITE, false, 0, 17.);
        assert_eq!(job.text.replace('\u{200b}', ""), "My notes");
        assert!(job.sections.iter().any(|s| s.format.font_id.size > 20.));
        assert_eq!(
            layout(source, 500., Color32::WHITE, true, 0, 17.).text,
            source
        );
    }

    #[test]
    fn layout_preserves_cursor_positions() {
        let text = "# 🦀\n**été**\n```\n# code\n```\n";
        for source in [false, true] {
            assert_eq!(
                layout(text, 400., Color32::WHITE, source, 0, 17.)
                    .text
                    .chars()
                    .count(),
                text.chars().count()
            );
        }
        assert_eq!(outline(text), [(0, "🦀".into())]);
    }
}
