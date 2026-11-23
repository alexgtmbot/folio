use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};

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
        viewer.show(ui, cache, &preview_text(text));
    });
}

pub fn editable_block(
    ui: &mut egui::Ui,
    cache: &mut egui_commonmark::CommonMarkCache,
    text: &str,
    r: &std::ops::Range<usize>,
    definitions: &[std::ops::Range<usize>],
    base: Option<&std::path::Path>,
    size: f32,
) -> bool {
    let source = crate::blocks::with_references(text, r, definitions);
    let response = ui
        .push_id(r.start, |ui| {
            preview(ui, cache, &source, base, size);
            ui.add_space(5.);
        })
        .response;
    let rect = response.rect;
    ui.input(|i| {
        i.pointer.primary_clicked()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| rect.contains(p) && ui.clip_rect().contains(p))
    })
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
                    Event::Code(_) => {
                        let raw = &text[r.clone()];
                        let ticks = raw.bytes().take_while(|b| *b == b'`').count();
                        let hidden = "\u{200b}".repeat(ticks);
                        job.append(
                            &format!("{hidden}{}{hidden}", &raw[ticks..raw.len() - ticks]),
                            0.,
                            f.clone(),
                        );
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
