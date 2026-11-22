use eframe::egui::{self, Color32, FontId, TextFormat, text::LayoutJob};
use folio::{Fence, block_prefix, inline};

pub fn outline(text: &str) -> Vec<(usize, String)> {
    let mut pos = 0;
    let mut fence = Fence::default();
    let mut result = Vec::new();
    for line in text.split_inclusive('\n') {
        let n = block_prefix(line);
        if !fence.contains(line) && n > 0 && line.starts_with('#') {
            result.push((pos, line[n..].trim().to_owned()));
        }
        pos += line.chars().count();
    }
    result
}

pub fn layout(text: &str, width: f32, color: Color32, source: bool, caret: usize) -> LayoutJob {
    let mut job = LayoutJob::default();
    job.wrap.max_width = width;
    let body = TextFormat {
        font_id: FontId::proportional(17.),
        color,
        ..Default::default()
    };
    let mono = TextFormat {
        font_id: FontId::monospace(15.),
        ..body.clone()
    };
    if source {
        job.append(text, 0., mono);
        return job;
    }
    let muted = TextFormat {
        font_id: FontId::proportional(0.5),
        color: Color32::TRANSPARENT,
        ..body.clone()
    };
    let mut fence = Fence::default();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let end = start + line.chars().count();
        let active = (start..end).contains(&caret) || end == caret && !line.ends_with('\n');
        if fence.contains(line) {
            job.append(line, 0., mono.clone());
            start = end;
            continue;
        }
        let prefix = block_prefix(line);
        let heading = prefix > 0 && line.starts_with('#');
        let face = if heading {
            TextFormat {
                font_id: FontId::proportional(34. - (prefix - 2) as f32 * 3.),
                ..body.clone()
            }
        } else {
            body.clone()
        };
        let mut pos = if heading { prefix } else { 0 };
        if heading {
            job.append(
                &line[..pos],
                0.,
                if active { body.clone() } else { muted.clone() },
            );
        }
        let rest = &line[pos..];
        pos = 0;
        inline(rest, |range, mark| {
            job.append(&rest[pos..range.start], 0., face.clone());
            let delimiter = if active { &face } else { &muted };
            job.append(mark, 0., delimiter.clone());
            let mut styled = face.clone();
            if mark == "`" {
                styled = mono.clone()
            } else if mark.len() == 1 {
                styled.italics = true
            } else {
                styled.font_id.family = egui::FontFamily::Name("Bold".into());
            }
            job.append(
                &rest[range.start + mark.len()..range.end - mark.len()],
                0.,
                styled,
            );
            job.append(mark, 0., delimiter.clone());
            pos = range.end;
        });
        job.append(&rest[pos..], 0., face);
        start = end;
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn layout_preserves_cursor_positions() {
        let text = "# 🦀\n**été**\n```\n# code\n```\n";
        for source in [false, true] {
            assert_eq!(layout(text, 400., Color32::WHITE, source, 0).text, text);
        }
        assert_eq!(outline(text), [(0, "🦀".into())]);
    }
}
