use folio::{FileFormat, block_prefix, task_prefix};
use std::{
    fs,
    io::{self, Write},
    ops::Range,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub struct Document {
    pub text: String,
    pub path: Option<PathBuf>,
    pub dirty: bool,
    format: FileFormat,
    last_saved: Vec<u8>,
}

impl Document {
    pub fn open(path: &Path) -> io::Result<Self> {
        let path = path.canonicalize()?;
        let bytes = fs::read(&path)?;
        let (text, format) = FileFormat::decode(&bytes).map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "This file is not UTF-8 Markdown.",
            )
        })?;
        Ok(Self {
            text,
            format,
            path: Some(path),
            last_saved: bytes,
            dirty: false,
        })
    }
    pub fn save(&mut self, destination: &Path) -> io::Result<()> {
        let path = if destination.exists() {
            destination.canonicalize()?
        } else {
            destination.to_owned()
        };
        if self.path.as_ref() == Some(&path) && fs::read(&path)? != self.last_saved {
            return Err(io::Error::other(
                "This file changed on disk. Use Save As to keep both versions.",
            ));
        }
        let bytes = self.format.encode(&self.text);
        let mut tmp = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
        if let Ok(meta) = fs::metadata(&path) {
            tmp.as_file().set_permissions(meta.permissions())?;
        }
        tmp.write_all(&bytes)?;
        tmp.as_file().sync_all()?;
        tmp.persist(&path)?;
        self.path = Some(path);
        self.last_saved = bytes;
        self.dirty = false;
        Ok(())
    }
    pub fn refresh_dirty(&mut self) {
        self.dirty = self.format.encode(&self.text) != self.last_saved;
    }
    pub fn name(&self) -> &str {
        self.path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|s| s.to_str())
            .unwrap_or("Untitled")
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Block(&'static str),
    Wrap(&'static str),
    CodeBlock,
    Task,
}

pub fn byte_at(text: &str, character: usize) -> usize {
    text.char_indices()
        .nth(character)
        .map_or(text.len(), |(i, _)| i)
}

/// Move beyond the closing delimiter when toggling the style at its trailing edge.
pub fn exit_inline(text: &str, selection: &Range<usize>, cmd: Format) -> Option<Range<usize>> {
    use pulldown_cmark::{Event, Parser, TagEnd};
    let Format::Wrap(mark) = cmd else { return None };
    if !selection.is_empty() {
        return None;
    }
    let caret = byte_at(text, selection.start);
    if !matches!(text.as_bytes().get(caret), Some(b'*' | b'_' | b'~' | b'`')) {
        return None;
    }
    for (event, span) in Parser::new_ext(text, crate::blocks::options()).into_offset_iter() {
        if !(span.start < caret && caret < span.end) {
            continue;
        }
        let suffix = &text[caret..span.end];
        let matches = match (mark, event) {
            ("**", Event::End(TagEnd::Strong)) => matches!(suffix, "**" | "__"),
            ("*", Event::End(TagEnd::Emphasis)) => matches!(suffix, "*" | "_"),
            ("~~", Event::End(TagEnd::Strikethrough)) => matches!(suffix, "~~" | "~"),
            ("`", Event::Code(_)) => {
                suffix.bytes().all(|b| b == b'`')
                    && suffix.len()
                        == text[span.start..span.end]
                            .bytes()
                            .take_while(|b| *b == b'`')
                            .count()
            }
            _ => false,
        };
        if matches {
            let after = selection.start + suffix.len();
            return Some(after..after);
        }
    }
    None
}

pub fn format(text: &mut String, selection: Range<usize>, cmd: Format) -> Range<usize> {
    let start = byte_at(text, selection.start);
    let end = byte_at(text, selection.end);
    if let Format::Block(prefix) = cmd
        && (prefix.is_empty() || prefix.starts_with('#'))
        && let Some(range) = crate::blocks::ranges(text)
            .into_iter()
            .find(|r| r.contains(&start) || start == text.len() && r.end == start)
    {
        let block = text[range.clone()].trim_end_matches(['\r', '\n']);
        if end <= range.start + block.len()
            && let Some((title, underline)) = block.rsplit_once('\n')
            && !underline.trim().is_empty()
            && (underline.trim().bytes().all(|b| b == b'=')
                || underline.trim().bytes().all(|b| b == b'-'))
            && matches!(
                pulldown_cmark::Parser::new(block).next(),
                Some(pulldown_cmark::Event::Start(
                    pulldown_cmark::Tag::Heading { .. }
                ))
            )
        {
            let title = title.trim_end_matches('\r');
            let offset = text[..range.start].chars().count();
            let remap = |cursor: usize| {
                offset + prefix.len() + (cursor - offset).min(title.chars().count())
            };
            let selection = remap(selection.start)..remap(selection.end);
            let title = if prefix.is_empty() {
                title.to_owned()
            } else {
                title.replace('\n', " ")
            };
            text.replace_range(
                range.start..range.start + block.len(),
                &format!("{prefix}{title}"),
            );
            return selection;
        }
    }
    match cmd {
        Format::CodeBlock => {
            let first = text[..start].rfind('\n').map_or(0, |i| i + 1);
            let last = if start < end && text[..end].ends_with('\n') {
                end - 1
            } else {
                end
            };
            let stop = text[last..].find('\n').map_or(text.len(), |i| last + i);
            let content = &text[first..stop];
            let longest = content
                .lines()
                .map(|line| line.trim_start().bytes().take_while(|b| *b == b'`').count())
                .max()
                .unwrap_or(0);
            let fence = "`".repeat(3.max(longest + 1));
            let shift = fence.len() + 1;
            let selected = selection.start + shift..text[..end.min(stop)].chars().count() + shift;
            text.insert_str(stop, &format!("\n{fence}"));
            text.insert_str(first, &format!("{fence}\n"));
            selected
        }
        Format::Wrap("`") if start < end => {
            let content = &text[start..end];
            for (event, span) in
                pulldown_cmark::Parser::new_ext(text, crate::blocks::options()).into_offset_iter()
            {
                if let pulldown_cmark::Event::Code(value) = event
                    && span.start < start
                    && end < span.end
                    && value.as_ref() == content
                    && text[span.start..start]
                        .trim_end_matches(' ')
                        .bytes()
                        .all(|b| b == b'`')
                    && text[end..span.end]
                        .trim_start_matches(' ')
                        .bytes()
                        .all(|b| b == b'`')
                {
                    let shift = start - span.start;
                    text.replace_range(end..span.end, "");
                    text.replace_range(span.start..start, "");
                    return selection.start - shift..selection.end - shift;
                }
            }
            let longest = content.split(|c| c != '`').map(str::len).max().unwrap_or(0);
            let ticks = "`".repeat(longest + 1);
            let pad = content.starts_with('`')
                || content.ends_with('`')
                || (content.starts_with(' ')
                    && content.ends_with(' ')
                    && !content.trim_matches(' ').is_empty());
            let space = if pad { " " } else { "" };
            text.insert_str(end, &format!("{space}{ticks}"));
            text.insert_str(start, &format!("{ticks}{space}"));
            let shift = ticks.len() + space.len();
            selection.start + shift..selection.end + shift
        }
        Format::Wrap(mark) => {
            let content = &text[start..end];
            let trimmed = content.trim();
            if !content.is_empty() && trimmed.is_empty() {
                return selection;
            }
            let leading = &content[..content.len() - content.trim_start().len()];
            let start = start + leading.len();
            let end = start + trimmed.len();
            let first = selection.start + leading.chars().count();
            let selection = first..first + trimmed.chars().count();
            if !selection.is_empty() {
                use pulldown_cmark::{Event, Parser, TagEnd};
                for (event, span) in
                    Parser::new_ext(text, crate::blocks::options()).into_offset_iter()
                {
                    let delimiters: &[&str] = match (mark, event) {
                        ("**", Event::End(TagEnd::Strong)) => &["**", "__"],
                        ("*", Event::End(TagEnd::Emphasis)) => &["*", "_"],
                        ("~~", Event::End(TagEnd::Strikethrough)) => &["~~", "~"],
                        _ => continue,
                    };
                    if let Some(delimiter) = delimiters.iter().find(|d| {
                        span.start + d.len() == start
                            && end + d.len() == span.end
                            && text[span.clone()].starts_with(**d)
                    }) {
                        text.replace_range(end..span.end, "");
                        text.replace_range(span.start..start, "");
                        return selection.start - delimiter.len()..selection.end - delimiter.len();
                    }
                }
            }
            if selection.is_empty()
                && text[..start].ends_with(mark)
                && text[end..].starts_with(mark)
            {
                text.replace_range(end..end + mark.len(), "");
                text.replace_range(start - mark.len()..start, "");
                return selection.start - mark.len()..selection.end - mark.len();
            }
            text.insert_str(end, mark);
            text.insert_str(start, mark);
            selection.start + mark.len()..selection.end + mark.len()
        }
        Format::Block(prefix @ ("- " | "1. " | "> ")) => {
            let first = text[..start].rfind('\n').map_or(0, |i| i + 1);
            let last = if end > start && text[..end].ends_with('\n') {
                end - 1
            } else {
                end
            };
            let stop = text[last..].find('\n').map_or(text.len(), |n| last + n);
            let mut output = String::new();
            let mut index = 0;
            let mut first_caret = 0;
            for (line_number, line) in text[first..stop].split('\n').enumerate() {
                if line_number > 0 {
                    output.push('\n');
                }
                if prefix == "> " {
                    output.push_str(if line.is_empty() && !selection.is_empty() {
                        ">"
                    } else {
                        "> "
                    });
                    if selection.is_empty() {
                        first_caret = 2 + text[first..start].chars().count();
                    }
                    output.push_str(line);
                    continue;
                }
                if !selection.is_empty() && line.trim().is_empty() {
                    output.push_str(line);
                    continue;
                }
                let body = line.trim_start_matches([' ', '\t']);
                let indent = line.len() - body.len();
                output.push_str(&line[..indent]);
                index += 1;
                if prefix == "1. " {
                    output.push_str(&format!("{index}. "));
                } else {
                    output.push_str(prefix);
                }
                // Changing list style replaces the bullet, preserving any checkbox after it.
                let content = body
                    .strip_prefix("- ")
                    .unwrap_or_else(|| &body[block_prefix(body)..]);
                if selection.is_empty() {
                    let content_start = first + line.len() - content.len();
                    let preceding = &text[content_start..start.max(content_start)];
                    first_caret = output.chars().count() + preceding.chars().count();
                }
                output.push_str(content);
            }
            let offset = text[..first].chars().count();
            let finish = offset + output.chars().count();
            if prefix == "> "
                && text[stop..]
                    .lines()
                    .nth(1)
                    .is_some_and(|line| !line.trim().is_empty())
            {
                // A following plain line would otherwise continue the final quoted paragraph.
                output.push('\n');
            }
            text.replace_range(first..stop, &output);
            if selection.is_empty() {
                let pos = offset + first_caret;
                pos..pos
            } else {
                offset..finish
            }
        }
        Format::Block(_) | Format::Task => {
            let pos = text[..start].rfind(['\r', '\n']).map_or(0, |i| i + 1);
            let mut pos = pos;
            if matches!(cmd, Format::Task) {
                let line = text[pos..].lines().next().unwrap_or_default();
                let body = line.trim_start_matches([' ', '\t']);
                pos += line.len() - body.len();
                if let Some((marker, content)) = body.split_once([' ', '\t'])
                    && (matches!(marker, "-" | "+" | "*")
                        || marker.strip_suffix(['.', ')']).is_some_and(|n| {
                            (1..=9).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit())
                        }))
                {
                    let content = content.trim_start_matches([' ', '\t']);
                    pos += body.len() - content.len();
                    if let Some(state) = content.as_bytes().get(1)
                        && matches!(state, b' ' | b'x' | b'X')
                        && content.starts_with('[')
                        && content.as_bytes().get(2) == Some(&b']')
                        && (content.len() == 3 || content[3..].starts_with([' ', '\t']))
                    {
                        text.replace_range(
                            pos + 1..pos + 2,
                            if *state == b' ' { "x" } else { " " },
                        );
                        return selection;
                    }
                    text.insert_str(pos, "[ ] ");
                    let at = text[..pos].chars().count();
                    return selection.start.max(at) + 4..selection.end.max(at) + 4;
                }
            }
            let line = &text[pos..];
            let (n, prefix) = match cmd {
                Format::Block(p) => (block_prefix(line), p),
                _ => task_prefix(line),
            };
            let line_start = text[..pos].chars().count();
            let mut selection = selection;
            if prefix.is_empty() && n > 0 && line.trim_start_matches(' ').starts_with('#') {
                let heading = line.lines().next().unwrap_or_default();
                let trimmed = heading.trim_end_matches([' ', '\t']);
                let before_hashes = trimmed.trim_end_matches('#');
                if before_hashes.len() < trimmed.len() && before_hashes.ends_with([' ', '\t']) {
                    let title_end = before_hashes.trim_end_matches([' ', '\t']).len().max(n);
                    let at = line_start + heading[..title_end].chars().count();
                    let removed = heading.len() - title_end;
                    let remap = |cursor: usize| cursor - cursor.saturating_sub(at).min(removed);
                    selection = remap(selection.start)..remap(selection.end);
                    text.replace_range(pos + title_end..pos + heading.len(), "");
                }
            }
            let remap =
                |cursor: usize| line_start + prefix.len() + cursor.saturating_sub(line_start + n);
            text.replace_range(pos..pos + n, prefix);
            remap(selection.start)..remap(selection.end)
        }
    }
}

/// Change the nesting of the list item at the caret, including its continuation lines.
pub fn nest_item(text: &mut String, caret: usize, lift: bool) -> Option<usize> {
    use pulldown_cmark::{Event, Parser, Tag, TagEnd};
    let byte = byte_at(text, caret);
    let mut lists = Vec::new();
    let mut parents = Vec::new();
    let mut found = None;
    for (e, r) in Parser::new_ext(text, crate::blocks::options()).into_offset_iter() {
        match e {
            Event::Start(Tag::List(_)) => lists.push(None),
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let previous = lists.last_mut()?.replace(r.start);
                if r.contains(&byte) || (byte == text.len() && r.end == byte) {
                    found = Some((r.clone(), previous, parents.last().copied()));
                }
                parents.push(r.start);
            }
            Event::End(TagEnd::Item) => {
                parents.pop();
            }
            Event::Start(Tag::CodeBlock(_)) if r.contains(&byte) => return None,
            _ => {}
        }
    }
    let (item, previous, parent) = found?;
    let line_start = |n| text[..n].rfind('\n').map_or(0, |i| i + 1);
    let start = line_start(item.start);
    let indent = &text[start..item.start];
    if !indent.bytes().all(|c| c == b' ') {
        return None;
    }
    let relative = if lift { parent? } else { previous? };
    let base = line_start(relative);
    if !text[base..relative].bytes().all(|c| c == b' ') {
        return None;
    }
    let width = if lift {
        indent.len().checked_sub(relative - base)?
    } else {
        let marker_end = text[relative..].find([' ', '\t'])? + relative;
        let padding = text[marker_end..]
            .bytes()
            .take_while(|c| *c == b' ')
            .count();
        if padding == 0 {
            return None;
        }
        (marker_end - base + if padding <= 4 { padding } else { 1 }).checked_sub(indent.len())?
    };
    if width == 0 {
        return None;
    }
    let mut replacement = String::new();
    let mut at = start;
    let mut new_byte = byte;
    // Empty items and ordered lists starting above one cannot interrupt a paragraph.
    let digits = text[item.start..]
        .bytes()
        .take_while(u8::is_ascii_digit)
        .count();
    let marker_line = text[item.start..].lines().next()?.trim_end();
    let empty_item = marker_line.len() == digits + 1;
    let ordered_continuation =
        digits > 0 && text[item.start..item.start + digits].parse::<u32>().ok() != Some(1);
    if !lift
        && (empty_item || ordered_continuation)
        && text[..start]
            .lines()
            .next_back()
            .is_some_and(|line| !line.trim().is_empty())
    {
        let newline = if text[..start].ends_with("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        replacement.push_str(newline);
        new_byte += newline.len();
    }
    for line in text[start..item.end].split_inclusive('\n') {
        if !line.trim().is_empty() {
            if lift {
                let remove = line.bytes().take(width).take_while(|c| *c == b' ').count();
                if at <= byte {
                    new_byte -= remove.min(byte - at);
                }
                replacement.push_str(&line[remove..]);
            } else {
                replacement.push_str(&" ".repeat(width));
                replacement.push_str(line);
                if at <= byte {
                    new_byte += width;
                }
            }
        } else {
            replacement.push_str(line)
        }
        at += line.len();
    }
    text.replace_range(start..item.end, &replacement);
    Some(text[..new_byte].chars().count())
}

pub fn exit_fenced_code(text: &mut String, block: Range<usize>) -> Option<usize> {
    use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag};
    let source = &text[block.clone()];
    let (Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_))), span) =
        Parser::new(source).into_offset_iter().next()?
    else {
        return None;
    };
    let raw = &source[span];
    let opening = raw.lines().next()?.trim_start();
    let marker = opening.chars().next()?;
    let fence = &opening[..opening.bytes().take_while(|b| *b == marker as u8).count()];
    let closed = raw
        .trim_end_matches(['\r', '\n'])
        .rsplit_once('\n')
        .is_some_and(|(_, last)| {
            let trimmed = last.trim_start_matches(' ');
            last.len() - trimmed.len() <= 3
                && trimmed.trim_end().len() >= fence.len()
                && trimmed.trim_end().bytes().all(|b| b == marker as u8)
        });
    let mut extra = String::new();
    if !closed {
        if !source.ends_with('\n') {
            extra.push('\n');
        }
        extra.push_str(fence);
        extra.push_str("\n\n");
    } else {
        extra.push_str(
            &"\n".repeat(
                2 - source
                    .bytes()
                    .rev()
                    .take(2)
                    .take_while(|b| *b == b'\n')
                    .count(),
            ),
        );
    }
    let caret = block.end + extra.len();
    if block.end < text.len() {
        extra.push_str("\n\n");
    }
    text.insert_str(block.end, &extra);
    Some(text[..caret].chars().count())
}

/// Return the character selection and replacement for structural Enter commands.
pub fn block_enter(text: &str, caret: usize) -> Option<(Range<usize>, String)> {
    use pulldown_cmark::{Event, Parser, Tag};
    let end = byte_at(text, caret);
    let start = text[..end].rfind('\n').map_or(0, |i| i + 1);
    let before = &text[start..end];
    let body = before.trim_start_matches([' ', '\t']);
    let indent = &before[..before.len() - body.len()];
    let rest_of_line = text[end..].split('\n').next()?;
    if !body.is_empty() {
        let mut depth = 0;
        let mut prose = false;
        let mut delimiter = "";
        for (event, span) in Parser::new_ext(text, crate::blocks::options()).into_offset_iter() {
            if span.start > end {
                break;
            }
            match event {
                Event::Start(Tag::Paragraph) if depth == 0 && span.contains(&(end - 1)) => {
                    if rest_of_line.is_empty() {
                        return Some((caret..caret, "\n\n".into()));
                    }
                    prose = true;
                    depth += 1;
                }
                Event::Text(_) if prose && depth == 1 && (span.start..=span.end).contains(&end) => {
                    return Some((caret..caret, "\n\n".into()));
                }
                Event::Start(tag @ (Tag::Strong | Tag::Emphasis | Tag::Strikethrough))
                    if prose && depth == 1 && span.contains(&end) =>
                {
                    let width = match tag {
                        Tag::Strong => 2,
                        Tag::Strikethrough => text[span.clone()]
                            .bytes()
                            .take_while(|&b| b == b'~')
                            .count(),
                        _ => 1,
                    };
                    delimiter = &text[span.start..span.start + width];
                    depth += 1;
                }
                Event::Text(value)
                    if depth == 2
                        && !delimiter.is_empty()
                        && span.start < end
                        && end < span.end
                        && value.as_ref() == &text[span.clone()] =>
                {
                    let left = text[span.start..end].trim_end_matches([' ', '\t']);
                    let right = text[end..span.end].trim_start_matches([' ', '\t']);
                    if !left.is_empty() && !right.is_empty() {
                        let from = caret - (end - span.start - left.len());
                        let to = caret + (span.end - end - right.len());
                        return Some((from..to, format!("{delimiter}\n\n{delimiter}")));
                    }
                }
                Event::End(_) if depth == 2 => {
                    delimiter = "";
                    depth -= 1;
                }
                Event::Start(_) => depth += 1,
                Event::End(_) => depth -= 1,
                _ => {}
            }
        }
    }
    if body.starts_with('>') {
        let mut prefix_end = indent.len();
        let mut last_marker = prefix_end;
        while before[prefix_end..].starts_with('>') {
            last_marker = prefix_end;
            prefix_end += 1;
            if before[prefix_end..].starts_with([' ', '\t']) {
                prefix_end += 1;
            }
        }
        let mut quote_start = None;
        for (event, span) in Parser::new_ext(text, crate::blocks::options()).into_offset_iter() {
            if !span.contains(&(start + last_marker)) {
                continue;
            }
            match event {
                Event::Start(Tag::BlockQuote(_)) => quote_start = Some(span.start),
                Event::Start(Tag::CodeBlock(_)) => return None,
                _ => {}
            }
        }
        let quote_start = quote_start?;
        if before[prefix_end..].trim().is_empty() && rest_of_line.trim().is_empty() {
            let line_start = caret - before.chars().count();
            let parent = &before[..last_marker];
            let separator = if quote_start < start {
                format!("\n{parent}")
            } else {
                String::new()
            };
            return Some((line_start + last_marker..caret, separator));
        }
        return Some((caret..caret, format!("\n{}", &before[..prefix_end])));
    }
    let (marker, content) = match body.split_once(' ') {
        Some(parts) => parts,
        None if rest_of_line.trim().is_empty() => (body, ""),
        None => return None,
    };
    fn next_marker(marker: &str) -> Option<String> {
        Some(match marker {
            "-" | "+" | "*" => marker.to_owned(),
            _ => {
                let digits = marker.strip_suffix(['.', ')'])?;
                if digits.is_empty()
                    || digits.len() > 9
                    || !digits.bytes().all(|b| b.is_ascii_digit())
                {
                    return None;
                }
                format!(
                    "{}{}",
                    digits.parse::<u32>().ok()?.checked_add(1)?,
                    &marker[digits.len()..]
                )
            }
        })
    }
    let next = next_marker(marker)?;
    let mut ancestors = Vec::new();
    let parent = Parser::new(text)
        .into_offset_iter()
        .find_map(|(event, range)| {
            match event {
                Event::Start(Tag::Item) => {
                    if (start..=start + indent.len()).contains(&range.start) {
                        return Some(ancestors.last().copied());
                    }
                    ancestors.push(range.start);
                }
                Event::End(pulldown_cmark::TagEnd::Item) => {
                    ancestors.pop();
                }
                _ => {}
            }
            None
        })?;
    let is_task = |content: &str| {
        ["[ ]", "[x]", "[X]"].iter().any(|marker| {
            content
                .strip_prefix(marker)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with(' '))
        })
    };
    let task = is_task(content);
    let empty = if task { &content[3..] } else { content };
    if empty.trim().is_empty() && rest_of_line.trim().is_empty() {
        let mut replacement = String::new();
        if let Some(parent) = parent {
            let line = text[..parent].rfind('\n').map_or(0, |i| i + 1);
            let prefix = &text[line..parent];
            if prefix.bytes().all(|b| b == b' ') {
                let parent_line = text[parent..].lines().next()?;
                let marker = parent_line.split_whitespace().next()?;
                replacement = format!("{prefix}{} ", next_marker(marker)?);
                if is_task(parent_line[marker.len()..].trim_start()) {
                    replacement.push_str("[ ] ");
                }
            }
        }
        return Some((caret - before.chars().count()..caret, replacement));
    }
    Some((
        caret..caret,
        format!("\n{indent}{next} {}", if task { "[ ] " } else { "" }),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn enter_after_top_level_prose_starts_a_markdown_paragraph() {
        let text = "Notes\n\nCafé **today**";
        let end = text.chars().count();
        assert_eq!(block_enter(text, end), Some((end..end, "\n\n".into())));
        for (text, caret) in [
            ("Café noir", 5),
            ("Café **today**", 5),
            ("**today** ends", 9),
        ] {
            assert_eq!(
                block_enter(text, caret),
                Some((caret..caret, "\n\n".into()))
            );
        }
        for (source, caret, expected) in [
            ("Use **bold** here", 8, "Use **bo**\n\n**ld** here"),
            ("_Café noir_", 6, "_Café_\n\n_noir_"),
            ("~~ab~~", 3, "~~a~~\n\n~~b~~"),
        ] {
            let (range, insert) = block_enter(source, caret).unwrap();
            let mut result = source.to_owned();
            result.replace_range(
                byte_at(source, range.start)..byte_at(source, range.end),
                &insert,
            );
            assert_eq!(result, expected);
        }
        let nested = "- item\n  continuation";
        assert!(block_enter(nested, nested.len()).is_none());
    }

    #[test]
    fn nest_and_lift_preserve_task_subtree_and_unicode_caret() {
        let original = "10. parent\n11. [x] café\n    - child\n12. last";
        let mut md = original.to_owned();
        let pos = md[..md.find("fé").unwrap()].chars().count();
        let moved = nest_item(&mut md, pos, false).unwrap();
        assert_eq!(
            md,
            "10. parent\n\n    11. [x] café\n        - child\n12. last"
        );
        assert_eq!(nest_item(&mut md, moved, true), Some(pos + 1));
        assert_eq!(md, original.replacen('\n', "\n\n", 1));
        assert!(nest_item(&mut md, 0, false).is_none());
        assert!(nest_item(&mut md, pos, true).is_none());
    }
    #[test]
    fn nesting_empty_bullet_keeps_parent_as_prose() {
        let mut md = String::from("- Hey\n  - Howdy\n  - ");
        let caret = md.chars().count();
        let moved = nest_item(&mut md, caret, false).unwrap();
        assert_eq!(md, "- Hey\n  - Howdy\n\n    - ");
        assert_eq!(moved, md.chars().count());
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(&md));
        assert_eq!(html.matches("<li>").count(), 3);
        assert!(html.contains("<p>Howdy</p>"));
    }

    #[test]
    fn enter_on_empty_child_continues_parent_list() {
        for (source, child, sibling) in [
            ("- Hey\n  - Howdy\n\n    - ", "    - ", "  - "),
            ("10. café\n\n    - ", "    - ", "11. "),
            ("- [X] Review\n\n  - ", "  - ", "- [ ] "),
            ("1. [ ] Review\n\n   - ", "   - ", "2. [ ] "),
        ] {
            let caret = source.chars().count();
            assert_eq!(
                block_enter(source, caret),
                Some((caret - child.len()..caret, sibling.into()))
            );
        }
    }

    #[test]
    fn enter_exits_empty_markers_after_trailing_spaces_are_stripped() {
        for marker in ["-", "+", "*", "3.", "3)", "- [ ]", "- [X]"] {
            assert_eq!(
                block_enter(marker, marker.len()),
                Some((0..marker.len(), String::new()))
            );
        }
        let nested = "- Parent\n\n  -";
        assert_eq!(
            block_enter(nested, nested.len()),
            Some((nested.len() - 3..nested.len(), "- ".into()))
        );
        assert_eq!(block_enter("Title\n-", 7), None);
        assert_eq!(block_enter("- content", 1), None);
    }

    #[test]
    fn list_conversion_preserves_a_caret_inside_the_item_text() {
        let mut md = "Trail\n  Café noir".to_owned();
        let caret = "Trail\n  Café".chars().count();
        let caret = format(&mut md, caret..caret, Format::Block("- "));
        assert_eq!(md, "Trail\n  - Café noir");
        assert_eq!(&md[byte_at(&md, caret.start)..], " noir");
        let caret = format(&mut md, caret, Format::Block("1. "));
        assert_eq!(md, "Trail\n  1. Café noir");
        assert_eq!(&md[byte_at(&md, caret.start)..], " noir");
    }

    #[test]
    fn switching_list_style_preserves_checked_and_unchecked_tasks() {
        let mut md = "- [x] café\n- [ ] tea".to_owned();
        let end = md.chars().count();
        let selection = format(&mut md, 0..end, Format::Block("1. "));
        assert_eq!(md, "1. [x] café\n2. [ ] tea");
        format(&mut md, selection, Format::Block("- "));
        assert_eq!(md, "- [x] café\n- [ ] tea");
    }

    #[test]
    fn task_format_preserves_list_marker_and_nesting() {
        for (source, expected) in [
            ("  * item", "  * [ ] item"),
            ("+ [ ] café", "+ [x] café"),
            ("  12) [X] done", "  12) [ ] done"),
        ] {
            let mut md = source.to_owned();
            let caret = md.chars().count();
            let selection = format(&mut md, caret..caret, Format::Task);
            assert_eq!(md, expected);
            assert_eq!(selection.start, expected.chars().count());
        }
    }

    #[test]
    fn restyling_underlined_headings_removes_the_old_marker() {
        let mut md = "Intro\n\ncafé\nnotes\n-----\n\nBody".to_owned();
        let selected = format(&mut md, 12..17, Format::Block("### "));
        assert_eq!(md, "Intro\n\n### café notes\n\nBody");
        assert_eq!(
            &md[byte_at(&md, selected.start)..byte_at(&md, selected.end)],
            "notes"
        );
        let mut md = "Title\n=====\n".to_owned();
        assert_eq!(format(&mut md, 7..7, Format::Block("")), 5..5);
        assert_eq!(md, "Title\n");
    }

    #[test]
    fn quote_wraps_selected_blocks_and_preserves_their_markdown() {
        let mut text = "before\n# Café\n\n- one\n- two\nafter".to_owned();
        let end = text[..text.find("after").unwrap()].chars().count();
        let selected = format(&mut text, 7..end, Format::Block("> "));
        assert_eq!(text, "before\n> # Café\n>\n> - one\n> - two\n\nafter");
        assert_eq!(
            &text[byte_at(&text, selected.start)..byte_at(&text, selected.end)],
            "> # Café\n>\n> - one\n> - two"
        );
        let mut text = "écho".to_owned();
        assert_eq!(format(&mut text, 2..2, Format::Block("> ")), 4..4);
        assert_eq!(text, "> écho");
    }

    #[test]
    fn converting_closed_headings_to_paragraphs_keeps_only_title_text() {
        for (source, expected) in [
            ("## Café ###  \nNext", "Café\nNext"),
            ("## ###", ""),
            ("## C#", "C#"),
            ("   ## Café ##", "Café"),
            ("    ## code", "    ## code"),
            ("  #literal", "  #literal"),
            (r"## literal \#", r"literal \#"),
        ] {
            let mut md = source.to_owned();
            let end = md.chars().count();
            let selection = format(&mut md, 0..end, Format::Block(""));
            assert_eq!(md, expected);
            assert_eq!(selection, 0..expected.chars().count());
        }
    }

    #[test]
    fn changing_heading_style_keeps_the_caret_on_the_same_text() {
        let mut tabbed = "##\t  Café noir".to_owned();
        assert_eq!(format(&mut tabbed, 9..9, Format::Block("")), 4..4);
        assert_eq!(tabbed, "Café noir");
        let mut md = "## Café noir".to_owned();
        let caret = format(&mut md, 7..7, Format::Block(""));
        assert_eq!(md, "Café noir");
        assert_eq!(caret, 4..4);
        let selected = format(&mut md, 5..9, Format::Block("### "));
        assert_eq!(md, "### Café noir");
        assert_eq!(
            &md[byte_at(&md, selected.start)..byte_at(&md, selected.end)],
            "noir"
        );
    }

    #[test]
    fn code_block_style_wraps_selected_lines_without_closing_on_literal_fences() {
        let mut text = "Intro\ncafé\n```\nfinish\nAfter".to_owned();
        let from = text[..text.find("café").unwrap()].chars().count();
        let end = text[..text.find("After").unwrap()].chars().count();
        let selected = format(&mut text, from..end, Format::CodeBlock);
        assert_eq!(text, "Intro\n````\ncafé\n```\nfinish\n````\nAfter");
        assert_eq!(
            &text[byte_at(&text, selected.start)..byte_at(&text, selected.end)],
            "café\n```\nfinish"
        );
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(&text));
        assert!(html.contains("<pre><code>café\n```\nfinish\n</code></pre>"));
        let mut blank = String::new();
        assert_eq!(format(&mut blank, 0..0, Format::CodeBlock), 4..4);
        assert_eq!(blank, "```\n\n```");
    }

    #[test]
    fn exiting_code_starts_a_paragraph_after_the_matching_fence() {
        for source in ["```rs\n🦀\n```", "~~~~\ncode\n~~~", "```"] {
            let mut text = source.to_owned();
            let end = text.len();
            let caret = exit_fenced_code(&mut text, 0..end).unwrap();
            text.insert_str(byte_at(&text, caret), "Outside");
            let mut html = String::new();
            pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(&text));
            assert!(html.ends_with("</code></pre>\n<p>Outside</p>\n"), "{html}");
        }
    }

    #[test]
    fn enter_continues_prose_quotes_and_exits_one_level_on_empty_lines() {
        for (text, expected) in [
            ("> café", "\n> "),
            ("> > nested", "\n> > "),
            (">tight", "\n>"),
        ] {
            let n = text.chars().count();
            assert_eq!(block_enter(text, n), Some((n..n, expected.into())));
        }
        assert_eq!(block_enter("> > ", 4), Some((2..4, String::new())));
        assert_eq!(block_enter("> ", 2), Some((0..2, String::new())));
        let quote = "> café\n> ";
        assert_eq!(block_enter(quote, 9), Some((7..9, "\n".into())));
        let literal = "```\n> literal";
        assert_eq!(block_enter(literal, literal.len()), None);
    }

    #[test]
    fn enter_handles_list_types_and_exits_empty_items() {
        for (line, next) in [
            ("- Hello", "\n- "),
            ("2. Yo", "\n3. "),
            ("- [x] café", "\n- [ ] "),
            ("- parent\n  * child", "\n  * "),
        ] {
            let n = line.chars().count();
            assert_eq!(block_enter(line, n), Some((n..n, next.into())));
        }
        assert_eq!(block_enter("- ", 2), Some((0..2, String::new())));
        assert_eq!(block_enter("```\n- literal", 13), None);
    }
    #[test]
    fn selection_toggles_the_existing_mark_regardless_of_delimiter() {
        for (source, mark, width) in [
            ("__café__", "**", 2),
            ("_café_", "*", 1),
            ("~~café~~", "~~", 2),
        ] {
            let mut text = source.to_owned();
            let selected = format(&mut text, width..width + 4, Format::Wrap(mark));
            assert_eq!(text, "café");
            assert_eq!(selected, 0..4);
        }
        let mut text = r"\*café*".to_owned();
        let selected = format(&mut text, 2..6, Format::Wrap("*"));
        assert_eq!(text, r"\**café**");
        assert_eq!(selected, 3..7);
        let mut html = String::new();
        pulldown_cmark::html::push_html(&mut html, pulldown_cmark::Parser::new(&text));
        assert_eq!(html.trim(), "<p>*<em>café</em>*</p>");
    }

    #[test]
    fn visual_marks_keep_selected_edge_whitespace_outside_delimiters() {
        for mark in ["**", "*", "~~"] {
            let mut text = "Say\u{a0}café now".to_owned();
            let selected = format(&mut text, 3..9, Format::Wrap(mark));
            assert_eq!(text, format!("Say\u{a0}{mark}café{mark} now"));
            assert_eq!(
                &text[byte_at(&text, selected.start)..byte_at(&text, selected.end)],
                "café"
            );
            format(&mut text, selected, Format::Wrap(mark));
            assert_eq!(text, "Say\u{a0}café now");
            assert_eq!(format(&mut text, 3..4, Format::Wrap(mark)), 3..4);
            assert_eq!(text, "Say\u{a0}café now");
        }
    }

    #[test]
    fn visual_inline_code_round_trips_literal_backticks_and_spaces() {
        for literal in ["call `x` now", "``🦀`", " padded ", "   ", "ordinary"] {
            let mut md = format!("Use {literal} here");
            let selection = 4..4 + literal.chars().count();
            let selected = format(&mut md, selection.clone(), Format::Wrap("`"));
            assert!(
                pulldown_cmark::Parser::new(&md).any(
                    |e| matches!(e, pulldown_cmark::Event::Code(value) if value.as_ref() == literal)
                ),
                "{md}"
            );
            assert_eq!(format(&mut md, selected, Format::Wrap("`")), selection);
            assert_eq!(md, format!("Use {literal} here"));
        }
    }

    #[test]
    fn toggle_off_after_typing_leaves_delimiters_intact() {
        for (mark, source) in [
            ("**", "__🦀__"),
            ("*", "*🦀*"),
            ("~~", "~~🦀~~"),
            ("`", "``🦀``"),
        ] {
            let n = source.find('🦀').unwrap() + 1;
            let end = source.chars().count();
            assert_eq!(
                exit_inline(source, &(n..n), Format::Wrap(mark)),
                Some(end..end)
            );
        }
        assert_eq!(
            exit_inline("x **literal", &(2..2), Format::Wrap("**")),
            None
        );
    }

    #[test]
    fn list_commands_convert_selected_lines_without_consuming_the_next_line() {
        let mut text = "before\n🦀\n\n- [x] café\n3) three\nafter".to_owned();
        let start = "before\n".chars().count();
        let end = text[..text.find("after").unwrap()].chars().count();
        let selection = format(&mut text, start..end, Format::Block("1. "));
        assert_eq!(text, "before\n1. 🦀\n\n2. [x] café\n3. three\nafter");
        format(&mut text, selection, Format::Block("- "));
        assert_eq!(text, "before\n- 🦀\n\n- [x] café\n- three\nafter");
    }
    #[test]
    fn save_as_redirects_subsequent_edits_to_the_copy() {
        let dir = tempfile::tempdir().unwrap();
        let original = dir.path().join("original.md");
        let copy = dir.path().join("copy.md");
        let bytes = "\u{feff}# café\r\n";
        fs::write(&original, bytes).unwrap();
        let mut doc = Document::open(&original).unwrap();
        doc.save(&copy).unwrap();
        assert_eq!(fs::read(&copy).unwrap(), bytes.as_bytes());
        doc.text.push_str("Revised copy\n");
        let active = doc.path.clone().unwrap();
        doc.save(&active).unwrap();
        assert_eq!(fs::read_to_string(&original).unwrap(), bytes);
        assert_eq!(
            fs::read_to_string(&copy).unwrap(),
            format!("{bytes}Revised copy\r\n")
        );
    }

    #[test]
    fn edit_save_and_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, "\u{feff}🦀 café\r\n").unwrap();
        let mut doc = Document::open(&path).unwrap();
        assert_eq!(format(&mut doc.text, 2..6, Format::Wrap("**")), 4..8);
        format(&mut doc.text, 0..0, Format::Block("## "));
        doc.save(&path).unwrap();
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "\u{feff}## 🦀 **café**\r\n"
        );
        fs::write(&path, "external edit").unwrap();
        assert!(doc.save(&path).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "external edit");
    }
}
