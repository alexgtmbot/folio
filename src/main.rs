#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod attachment;
mod blocks;
mod document;
mod history;
mod link;
#[cfg(target_os = "macos")]
mod mac_open;
#[cfg(target_os = "macos")]
mod mac_png;
mod slash;
mod table;
mod window;

use document::{Document, Format, byte_at};
use eframe::egui::{
    self, Key, Modifiers, TextEdit,
    text::{CCursor, CCursorRange},
};
use std::{
    ops::Range,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

struct Tab {
    id: u64,
    doc: Document,
    selection: Range<usize>,
    drag_anchor: Option<usize>,
    source: bool,
    preview: bool,
    slash_index: usize,
    dismissed: Option<(usize, String)>,
    edited: Option<Instant>,
    edit_block: bool,
    history: history::History,
    recorded: Option<Instant>,
    table_edit: Option<(Range<usize>, String)>,
}
impl Tab {
    fn rendered_block(
        &mut self,
        ui: &mut egui::Ui,
        cache: &mut egui_commonmark::CommonMarkCache,
        r: &Range<usize>,
        definitions: &[Range<usize>],
        size: f32,
    ) -> Option<Range<usize>> {
        let base = self.doc.path.as_deref().and_then(|p| p.parent());
        match window::editable_block(ui, cache, &self.doc.text, r, definitions, base, size) {
            window::BlockAction::DragStart(byte) => {
                self.drag_anchor = Some(self.doc.text[..byte].chars().count());
                None
            }
            window::BlockAction::DragEnd(byte) => {
                let anchor = self.drag_anchor.take()?;
                ui.ctx().request_repaint();
                let end = self.doc.text[..byte].chars().count();
                Some(anchor.min(end)..anchor.max(end))
            }
            window::BlockAction::Edit(byte) => {
                let pos = self.doc.text[..byte].chars().count();
                ui.ctx().request_repaint();
                Some(pos..pos)
            }
            window::BlockAction::Task(byte, checked) => {
                self.doc
                    .text
                    .replace_range(byte..byte + 1, if checked { "x" } else { " " });
                self.doc.dirty = true;
                self.edited = Some(Instant::now());
                None
            }
            window::BlockAction::ReplaceTable(range, updated) => {
                self.table_edit = Some((range, updated));
                None
            }
            window::BlockAction::None => None,
        }
    }
}
#[derive(Clone, Copy)]
enum Closing {
    Tab,
    App,
}

fn format_shortcuts() -> [(Modifiers, Key, Format); 13] {
    let cmd = Modifiers::COMMAND;
    let shifted = cmd | Modifiers::SHIFT;
    [
        (cmd, Key::B, Format::Wrap("**")),
        (cmd, Key::I, Format::Wrap("*")),
        (cmd, Key::E, Format::Wrap("`")),
        (cmd, Key::Num0, Format::Block("")),
        (cmd, Key::Num1, Format::Block("# ")),
        (cmd, Key::Num2, Format::Block("## ")),
        (cmd, Key::Num3, Format::Block("### ")),
        (cmd, Key::Num4, Format::Block("#### ")),
        (cmd, Key::Num5, Format::Block("##### ")),
        (cmd, Key::Num6, Format::Block("###### ")),
        (shifted, Key::Num7, Format::Block("1. ")),
        (shifted, Key::Num8, Format::Block("- ")),
        (shifted, Key::T, Format::Task),
    ]
}

fn shortcut_label(ctx: &egui::Context, action: Format) -> Option<String> {
    format_shortcuts()
        .into_iter()
        .find_map(|(modifiers, key, format)| {
            (format == action)
                .then(|| ctx.format_shortcut(&egui::KeyboardShortcut::new(modifiers, key)))
        })
}

struct Editor {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    window_title: String,
    preview_cache: egui_commonmark::CommonMarkCache,
    closing: Option<Closing>,
    quit: bool,
    error: Option<String>,
    find: Option<String>,
    focus_find: bool,
    link: Option<link::Link>,
    show_outline: bool,
    line_width: f32,
    text_size: f32,
    jump: Option<Range<usize>>,
}

impl Editor {
    fn new() -> Self {
        let paths = std::env::args_os().skip(1).map(PathBuf::from);
        #[cfg(target_os = "macos")]
        let paths = paths.chain(mac_open::pending());
        Self::with_documents(paths)
    }

    fn with_documents(paths: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            window_title: String::new(),
            preview_cache: Default::default(),
            closing: None,
            quit: false,
            error: None,
            find: None,
            focus_find: false,
            link: None,
            show_outline: false,
            line_width: 730.,
            text_size: 17.,
            jump: None,
        };
        for path in paths {
            app.open(&path);
        }
        if app.tabs.is_empty() {
            app.add(Document::default());
        }
        app
    }
    fn add(&mut self, doc: Document) {
        let edit_block = doc.text.is_empty();
        self.tabs.push(Tab {
            edit_block,
            history: history::History::new(&doc.text),
            recorded: None,
            table_edit: None,
            id: self.next_id,
            doc,
            source: false,
            preview: false,
            slash_index: 0,
            dismissed: None,
            selection: 0..0,
            drag_anchor: None,
            edited: None,
        });
        self.next_id += 1;
        self.active = self.tabs.len() - 1;
        self.jump = edit_block.then_some(0..0);
    }
    fn open(&mut self, path: &Path) {
        match Document::open(path) {
            Ok(doc) => {
                if let Some(i) = self.tabs.iter().position(|t| t.doc.path == doc.path) {
                    if self.active != i {
                        self.active = i;
                        self.jump = self.tabs[i]
                            .edit_block
                            .then(|| self.tabs[i].selection.clone());
                    }
                } else {
                    self.add(doc);
                }
            }
            Err(e) => self.error = Some(format!("Could not open {}. {e}", path.display())),
        }
    }
    fn open_dialog(&mut self) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Markdown", &["md", "markdown", "txt"])
            .add_filter("All files", &["*"])
            .pick_file()
        {
            self.open(&path);
        }
    }
    fn save(&mut self, as_new: bool) -> bool {
        let tab = &mut self.tabs[self.active];
        let path = if !as_new { tab.doc.path.clone() } else { None };
        let Some(path) = path.or_else(|| {
            rfd::FileDialog::new()
                .set_file_name(
                    tab.doc
                        .path
                        .as_ref()
                        .and_then(|p| p.file_name())
                        .and_then(|s| s.to_str())
                        .unwrap_or("Untitled.md"),
                )
                .save_file()
        }) else {
            return false;
        };
        tab.edited = None;
        match tab.doc.save(&path) {
            Ok(()) => true,
            Err(e) => {
                self.error = Some(format!("Could not save {}. {e}", path.display()));
                false
            }
        }
    }
    fn apply(&mut self, cmd: Format) {
        let tab = &mut self.tabs[self.active];
        if let Some(after) = document::exit_inline(&tab.doc.text, &tab.selection, cmd) {
            self.jump = Some(after);
            return;
        }
        self.jump = Some(document::format(
            &mut tab.doc.text,
            tab.selection.clone(),
            cmd,
        ));
        tab.doc.dirty = true;
        tab.edited = Some(Instant::now());
    }
    fn search(&mut self) {
        let Some(query) = self.find.as_ref().filter(|q| !q.is_empty()) else {
            return;
        };
        let tab = &self.tabs[self.active];
        let text = &tab.doc.text;
        let start = byte_at(text, tab.selection.end);
        let mut needle = query.chars();
        let first = needle.next().expect("nonempty search query");
        let same_letter = |a: char, b: char| a == b || a.to_lowercase().eq(b.to_lowercase());
        let matches_from = |(pos, ch): (usize, char)| {
            if !same_letter(ch, first) {
                return false;
            }
            let mut rest = text[pos + ch.len_utf8()..].chars();
            needle
                .clone()
                .all(|q| rest.next().is_some_and(|c| same_letter(c, q)))
        };
        if let Some((pos, _)) = text[start..]
            .char_indices()
            .map(|(pos, ch)| (start + pos, ch))
            .chain(text[..start].char_indices())
            .find(|&candidate| matches_from(candidate))
        {
            let n = text[..pos].chars().count();
            self.jump = Some(n..n + query.chars().count());
        } else {
            self.error = Some("No matching text.".into());
        }
    }
    fn request_close_tab(&mut self) {
        if self.tabs[self.active].doc.dirty {
            self.closing = Some(Closing::Tab);
        } else {
            self.close_tab();
        }
    }

    fn close_tab(&mut self) {
        self.tabs.remove(self.active);
        if self.tabs.is_empty() {
            self.add(Document::default());
        } else {
            self.active = self.active.min(self.tabs.len() - 1);
            self.jump = None;
        }
    }
    fn finish_close(&mut self, ctx: &egui::Context, kind: Closing) {
        self.closing = None;
        match kind {
            Closing::Tab => self.close_tab(),
            Closing::App => {
                if let Some(i) = self.tabs.iter().position(|t| t.doc.dirty) {
                    self.active = i;
                    self.closing = Some(kind);
                } else {
                    self.quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }
    fn handle_drops(&mut self, ctx: &egui::Context) {
        let drops: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .filter_map(|f| f.path.clone())
                .collect()
        });
        let (images, docs): (Vec<_>, Vec<_>) =
            drops.into_iter().partition(|p| attachment::supported(p));
        self.insert_images(images.into_iter().map(attachment::Image::File).collect());
        for path in docs {
            self.open(&path);
        }
    }
    fn insert_images(&mut self, images: Vec<attachment::Image>) {
        if images.is_empty() {
            return;
        }
        if self.tabs[self.active].doc.path.is_none() && !self.save(false) {
            return;
        }
        let tab = &mut self.tabs[self.active];
        let mut inserted = Vec::new();
        for image in images {
            match attachment::store(tab.doc.path.as_deref().unwrap(), image) {
                Ok(md) => inserted.push(md),
                Err(e) => {
                    self.error = Some(format!("Could not insert image. {e}"));
                    break;
                }
            }
        }
        if inserted.is_empty() {
            return;
        }
        let markdown = inserted.join("\n\n");
        let start = byte_at(&tab.doc.text, tab.selection.start);
        let end = byte_at(&tab.doc.text, tab.selection.end);
        let caret = tab.doc.text[..start].chars().count() + markdown.chars().count();
        tab.doc.text.replace_range(start..end, &markdown);
        tab.selection = caret..caret;
        if tab.source {
            self.jump = Some(tab.selection.clone());
        } else {
            tab.edit_block = false;
            self.jump = None;
        }
        tab.doc.dirty = true;
        tab.edited = Some(Instant::now());
    }

    fn paste_link(&mut self, clipboard: &str) -> bool {
        let tab = &mut self.tabs[self.active];
        if tab.source || tab.selection.is_empty() {
            return false;
        }
        let range =
            byte_at(&tab.doc.text, tab.selection.start)..byte_at(&tab.doc.text, tab.selection.end);
        let Some(markdown) = link::paste_over_selection(&tab.doc.text, range.clone(), clipboard)
        else {
            return false;
        };
        let caret = tab.selection.start + markdown.chars().count();
        tab.doc.text.replace_range(range, &markdown);
        self.jump = Some(caret..caret);
        tab.selection = caret..caret;
        tab.edited = Some(Instant::now());
        tab.doc.dirty = true;
        true
    }

    fn insert_link(&mut self, image: bool) {
        let tab = &self.tabs[self.active];
        let range =
            byte_at(&tab.doc.text, tab.selection.start)..byte_at(&tab.doc.text, tab.selection.end);
        self.link = Some(link::Link::at_selection(
            tab.id,
            &tab.doc.text,
            range,
            image,
        ));
    }

    fn move_blocks(&mut self, up: bool) {
        let tab = &mut self.tabs[self.active];
        if let Some(range) = blocks::move_selection(&mut tab.doc.text, &tab.selection, up) {
            tab.selection = range.clone();
            self.jump = Some(range);
            tab.doc.dirty = true;
            tab.edited = Some(Instant::now());
        }
    }
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let key = |m, k| ctx.input_mut(|i| i.consume_key(m, k));
        let cmd = Modifiers::COMMAND;
        let shifted = cmd | Modifiers::SHIFT;
        if !ctx.memory(|m| m.has_focus(egui::Id::new("find query"))) {
            let up = key(cmd | Modifiers::ALT, Key::ArrowUp);
            if up || key(cmd | Modifiers::ALT, Key::ArrowDown) {
                self.move_blocks(up);
            }
            let redo = key(shifted, Key::Z) || (!cfg!(target_os = "macos") && key(cmd, Key::Y));
            if redo || key(cmd, Key::Z) {
                let tab = &mut self.tabs[self.active];
                if let Some(selection) = tab.history.restore(&mut tab.doc.text, redo) {
                    self.jump = Some(selection);
                    tab.doc.refresh_dirty();
                    tab.edited = tab.doc.dirty.then(Instant::now);
                    tab.recorded = tab.edited;
                }
            }
        }
        if key(cmd | Modifiers::ALT, Key::K) {
            let tab = &self.tabs[self.active];
            if let Some(url) =
                link::web_link_at(&tab.doc.text, byte_at(&tab.doc.text, tab.selection.start))
            {
                ctx.open_url(egui::OpenUrl::new_tab(url));
            }
        } else if key(cmd, Key::K) {
            self.insert_link(false);
        }
        if !self.tabs[self.active].edit_block
            && !self.tabs[self.active].source
            && self.find.is_none()
            && key(cmd, Key::A)
        {
            self.jump = Some(0..self.tabs[self.active].doc.text.chars().count());
        }
        if key(cmd, Key::F) {
            self.find = Some(String::new());
            self.focus_find = true;
        }
        if key(cmd | Modifiers::ALT, Key::P) {
            let tab = &mut self.tabs[self.active];
            self.jump = Some(tab.selection.clone());
            tab.preview = tab.source || !tab.preview;
            tab.source = false;
        } else if key(cmd, Key::Slash) {
            let tab = &mut self.tabs[self.active];
            self.jump = Some(tab.selection.clone());
            tab.source = !tab.source;
        }
        for (m, k, f) in format_shortcuts() {
            if key(m, k) {
                self.apply(f);
            }
        }
    }
    fn finish_document_shortcuts(&mut self, ctx: &egui::Context) {
        let save_as =
            ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND | Modifiers::SHIFT, Key::S));
        if save_as || ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::S)) {
            self.save(save_as);
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::N)) {
            self.add(Document::default());
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::O)) {
            self.open_dialog();
        }
        if ctx.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::W)) {
            self.request_close_tab();
        }
    }
    fn insert_block(&mut self, command: &slash::Command) {
        let tab = &mut self.tabs[self.active];
        let byte = byte_at(&tab.doc.text, tab.selection.end);
        let end = tab.doc.text[byte..]
            .find('\n')
            .map_or(tab.doc.text.len(), |n| byte + n);
        let prefix = if end == 0 || tab.doc.text[..end].ends_with("\n\n") {
            ""
        } else {
            "\n\n"
        };
        let insert = end + prefix.len();
        tab.doc.text.insert_str(end, prefix);
        let caret = slash::insert(&mut tab.doc.text, insert..insert, command);
        let suffix = byte_at(&tab.doc.text, caret);
        if let Some(n) = tab.doc.text[suffix..].find('\n') {
            tab.doc.text.insert(suffix + n, '\n');
        }
        self.jump = Some(caret..caret);
        tab.doc.dirty = true;
        tab.edited = Some(Instant::now());
    }
    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("commands").show(ctx, |ui| {
            ui.add_space(4.);
            ui.horizontal(|ui| {
                ui.style_mut().visuals.button_frame = false;
                ui.spacing_mut().button_padding = egui::vec2(8., 5.);
                ui.label(egui::RichText::new("Folio").strong().size(19.));
                ui.separator();
                ui.toggle_value(&mut self.show_outline, "Outline");
                ui.separator();
                ui.menu_button("File", |ui| {
                    for (name, action) in [
                        ("New", 0),
                        ("Open…", 1),
                        ("Save", 2),
                        ("Save As…", 3),
                        ("Close document", 4),
                    ] {
                        if ui.button(name).clicked() {
                            ui.close_menu();
                            match action {
                                0 => self.add(Document::default()),
                                1 => self.open_dialog(),
                                2 | 3 => {
                                    self.save(action == 3);
                                }
                                _ => {
                                    self.request_close_tab();
                                }
                            }
                        }
                    }
                });
                ui.menu_button("Style", |ui| {
                    ui.label("Appearance");
                    egui::widgets::global_theme_preference_buttons(ui);
                    ui.add(egui::Slider::new(&mut self.text_size, 13.0..=26.0).text("Text size"));
                    ui.add(
                        egui::Slider::new(&mut self.line_width, 480.0..=1050.).text("Page width"),
                    );
                });
                if ui.button("Find").clicked() {
                    self.find = Some(String::new());
                    self.focus_find = true;
                }
                let tab = &mut self.tabs[self.active];
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let previous = (tab.source, tab.preview);
                    ui.selectable_value(&mut tab.source, true, "Source");
                    if ui
                        .selectable_label(!tab.source && tab.preview, "Split")
                        .clicked()
                    {
                        tab.source = false;
                        tab.preview = true;
                    }
                    if ui
                        .selectable_label(!tab.source && !tab.preview, "Write")
                        .clicked()
                    {
                        tab.source = false;
                        tab.preview = false;
                    }
                    if previous != (tab.source, tab.preview) {
                        self.jump = Some(tab.selection.clone());
                    }
                });
            });
            ui.add_space(4.);
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.add_space(8.);
                ui.spacing_mut().interact_size.y = 29.;
                ui.visuals_mut().button_frame = false;
                ui.spacing_mut().button_padding = egui::vec2(8., 5.);
                ui.menu_button("Text style", |ui| {
                    for (name, prefix) in [
                        ("Paragraph", ""),
                        ("Heading 1", "# "),
                        ("Heading 2", "## "),
                        ("Heading 3", "### "),
                        ("Heading 4", "#### "),
                        ("Heading 5", "##### "),
                        ("Heading 6", "###### "),
                    ] {
                        let action = Format::Block(prefix);
                        let button = egui::Button::new(name)
                            .shortcut_text(shortcut_label(ctx, action).unwrap_or_default());
                        if ui.add(button).clicked() {
                            self.apply(action);
                            ui.close_menu();
                        }
                    }
                    if ui.button("Code block").clicked() {
                        self.apply(Format::CodeBlock);
                        ui.close_menu();
                    }
                    if ui.button(slash::DIVIDER.name).clicked() {
                        self.insert_block(&slash::DIVIDER);
                        ui.close_menu();
                    }
                    ui.separator();
                    for (name, up) in [("Move block up", true), ("Move block down", false)] {
                        if ui.button(name).clicked() {
                            self.move_blocks(up);
                            ui.close_menu();
                        }
                    }
                });
                ui.separator();
                for (caption, help, command) in [
                    (
                        egui::RichText::new("B").strong(),
                        "Bold",
                        Format::Wrap("**"),
                    ),
                    (
                        egui::RichText::new("I").italics(),
                        "Italic",
                        Format::Wrap("*"),
                    ),
                    (
                        egui::RichText::new("S").strikethrough(),
                        "Strikethrough",
                        Format::Wrap("~~"),
                    ),
                    (
                        egui::RichText::new("</>").monospace(),
                        "Inline code",
                        Format::Wrap("`"),
                    ),
                    (
                        egui::RichText::new("Quote"),
                        "Block quote",
                        Format::Block("> "),
                    ),
                    (
                        egui::RichText::new("• List"),
                        "Bullet list",
                        Format::Block("- "),
                    ),
                    (
                        egui::RichText::new("1. List"),
                        "Numbered list",
                        Format::Block("1. "),
                    ),
                    (egui::RichText::new("Task"), "Toggle task", Format::Task),
                ] {
                    if ui
                        .add(egui::Button::new(caption.size(15.)).min_size(egui::vec2(32., 30.)))
                        .on_hover_text(
                            shortcut_label(ctx, command)
                                .map_or_else(|| help.to_owned(), |keys| format!("{help} ({keys})")),
                        )
                        .clicked()
                    {
                        self.apply(command);
                    }
                }
                ui.separator();
                if ui.button("Table").clicked() {
                    self.insert_block(&slash::TABLE);
                }
                if ui.button("Image").clicked() {
                    self.insert_link(true);
                }
                if ui.button("Link").clicked() {
                    self.insert_link(false);
                }
            });
            ui.add_space(3.);
            if self.tabs.len() > 1 {
                egui::ScrollArea::horizontal().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        for (i, tab) in self.tabs.iter().enumerate() {
                            if ui
                                .selectable_label(i == self.active, tab.doc.name())
                                .clicked()
                            {
                                self.active = i;
                                self.jump = None;
                            }
                        }
                    });
                });
            }
            if self.find.is_some() {
                ui.horizontal(|ui| {
                    let response = ui.add(
                        TextEdit::singleline(self.find.as_mut().unwrap())
                            .id(egui::Id::new("find query")),
                    );
                    if self.focus_find {
                        response.request_focus();
                        self.focus_find = false;
                    }
                    if ui.button("Find next").clicked()
                        || response.lost_focus()
                            && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
                    {
                        self.search();
                    }
                    if ui.button("Done").clicked() {
                        self.find = None;
                    }
                });
            }
        });
    }
    fn editor(&mut self, ctx: &egui::Context) {
        let mut bubble_action = None;
        let tab = &mut self.tabs[self.active];
        if ctx.input(|i| i.pointer.primary_pressed()) {
            tab.drag_anchor = None;
        }
        let document_id = egui::Id::new(("editor", tab.id));
        if self.show_outline {
            egui::SidePanel::left("outline")
                .default_width(155.)
                .show(ctx, |ui| {
                    ui.add_space(10.);
                    ui.strong("Outline");
                    ui.separator();
                    egui::ScrollArea::vertical().show(ui, |ui| {
                        let headings = window::outline(&tab.doc.text);
                        if headings.is_empty() {
                            ui.weak("No headings yet");
                        }
                        for (pos, level, title) in headings {
                            ui.horizontal(|ui| {
                                ui.add_space((level - 1) as f32 * 12.0);
                                if ui.selectable_label(false, title).clicked() {
                                    self.jump = Some(pos..pos);
                                }
                            });
                        }
                    });
                });
        }
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                let count = blocks::word_count(&tab.doc.text);
                ui.small(format!(
                    "{} {}",
                    count,
                    if count == 1 { "word" } else { "words" }
                ));
            });
        });
        if !tab.source && tab.preview {
            let render = |ui: &mut egui::Ui| {
                let ink = egui::Color32::from_rgb(214, 225, 237);
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(27, 37, 51))
                    .inner_margin(egui::Margin::same(16))
                    .show(ui, |ui| {
                        ui.set_min_size(ui.available_size());
                        ui.label(egui::RichText::new("Markdown").strong().color(ink));
                        ui.add_space(10.);
                        egui::ScrollArea::both()
                            .id_salt(("source output", tab.id))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&tab.doc.text)
                                            .monospace()
                                            .color(ink)
                                            .size(self.text_size * 0.85),
                                    )
                                    .extend()
                                    .selectable(true),
                                );
                            });
                    });
            };
            if ctx.available_rect().width() > 610. {
                egui::SidePanel::right("preview")
                    .frame(egui::Frame::NONE)
                    .default_width(ctx.available_rect().width() * 0.47)
                    .show(ctx, render);
            } else {
                egui::TopBottomPanel::bottom("preview")
                    .resizable(true)
                    .default_height(ctx.available_rect().height() * 0.4)
                    .show(ctx, render);
            }
        }
        egui::CentralPanel::default()
            .frame(
                egui::Frame::central_panel(&ctx.style())
                    .inner_margin(egui::Margin::symmetric(24, 12)),
            )
            .show(ctx, |ui| {
                let target = self.jump.take();
                let visual = !tab.source;
                let ranges = if visual {
                    blocks::ranges(&tab.doc.text)
                } else {
                    std::iter::once(0..tab.doc.text.len()).collect()
                };
                if let Some(r) = &target {
                    tab.selection = r.clone();
                    tab.edit_block = true;
                }
                let selection = target.as_ref().unwrap_or(&tab.selection);
                let start = byte_at(&tab.doc.text, selection.start);
                let end = byte_at(
                    &tab.doc.text,
                    selection
                        .end
                        .saturating_sub(usize::from(!selection.is_empty())),
                );
                let first = ranges
                    .partition_point(|r| r.end <= start)
                    .min(ranges.len() - 1);
                let last = ranges
                    .partition_point(|r| r.end <= end)
                    .min(ranges.len() - 1);
                let active = ranges[first].start..ranges[last].end;
                let mut editable = active.clone();
                if visual && last + 1 < ranges.len() {
                    let body_end = active.start
                        + tab.doc.text[active.clone()]
                            .trim_end_matches(['\r', '\n'])
                            .len();
                    if byte_at(&tab.doc.text, selection.end) <= body_end {
                        editable.end = body_end;
                    }
                }
                let offset = tab.doc.text[..active.start].chars().count();
                let id = document_id.with((visual, active.start));
                let definitions: Vec<_> = if visual {
                    pulldown_cmark::Parser::new_ext(&tab.doc.text, blocks::options())
                        .reference_definitions()
                        .iter()
                        .map(|(_, def)| def.span.clone())
                        .collect()
                } else {
                    vec![]
                };
                if let Some(r) = &target {
                    let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
                    state.cursor.set_char_range(Some(CCursorRange::two(
                        CCursor::new(r.start - offset),
                        CCursor::new(r.end - offset),
                    )));
                    state.store(ctx, id);
                    ctx.memory_mut(|m| m.request_focus(id));
                }
                let gutter = ((ui.available_width() - self.line_width) * 0.5).max(0.);
                ui.scope_builder(
                    egui::UiBuilder::new().max_rect(ui.max_rect().shrink2(egui::vec2(gutter, 0.))),
                    |ui| {
                        egui::ScrollArea::vertical().id_salt(tab.id).show(ui, |ui| {
                            ui.add_space(14.);
                            if visual && !tab.edit_block {
                                for r in &ranges {
                                    if let Some(selection) = tab.rendered_block(
                                        ui,
                                        &mut self.preview_cache,
                                        r,
                                        &definitions,
                                        self.text_size,
                                    ) {
                                        self.jump = Some(selection);
                                    }
                                }
                                if ui
                                    .allocate_response(
                                        egui::vec2(ui.available_width(), 40.),
                                        egui::Sense::click(),
                                    )
                                    .clicked()
                                {
                                    let end = tab.doc.text.chars().count();
                                    self.jump = Some(end..end);
                                }
                                return;
                            }
                            for r in &ranges[..first] {
                                if let Some(selection) = tab.rendered_block(
                                    ui,
                                    &mut self.preview_cache,
                                    r,
                                    &definitions,
                                    self.text_size,
                                ) {
                                    self.jump = Some(selection);
                                }
                            }
                            if visual {
                                let mut content = tab.doc.text[active.clone()].to_owned();
                                let focus = target.as_ref().map(|r| {
                                    byte_at(&content, r.start.saturating_sub(offset))
                                        ..byte_at(&content, r.end.saturating_sub(offset))
                                });
                                if let Some((changed, caret)) = ui
                                    .push_id(id, |ui| {
                                        table::edit(ui, &mut content, focus, self.text_size)
                                    })
                                    .inner
                                {
                                    let delta = content.len() as isize - active.len() as isize;
                                    if changed {
                                        tab.doc.text.replace_range(active.clone(), &content);
                                        tab.doc.dirty = true;
                                        tab.edited = Some(Instant::now());
                                    }
                                    if let Some(r) = caret {
                                        tab.selection = offset + content[..r.start].chars().count()
                                            ..offset + content[..r.end].chars().count();
                                    }
                                    if ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Escape))
                                    {
                                        tab.edit_block = false;
                                    }
                                    if ui.input_mut(|i| {
                                        i.consume_key(Modifiers::COMMAND, Key::Enter)
                                    }) {
                                        let end = active.start + content.len();
                                        let extra = 2 - content
                                            .chars()
                                            .rev()
                                            .take(2)
                                            .take_while(|c| *c == '\n')
                                            .count();
                                        tab.doc.text.insert_str(end, &"\n".repeat(extra));
                                        let pos = tab.doc.text[..end + extra].chars().count();
                                        self.jump = Some(pos..pos);
                                        tab.doc.dirty = true;
                                        tab.edited = Some(Instant::now());
                                        ctx.request_repaint();
                                        return;
                                    }
                                    for old in &ranges[last + 1..] {
                                        let r = old.start.saturating_add_signed(delta)
                                            ..old.end.saturating_add_signed(delta);
                                        let refs = definitions
                                            .iter()
                                            .filter(|d| !active.contains(&d.start))
                                            .map(|d| {
                                                if d.start >= active.end {
                                                    d.start.saturating_add_signed(delta)
                                                        ..d.end.saturating_add_signed(delta)
                                                } else {
                                                    d.clone()
                                                }
                                            })
                                            .collect::<Vec<_>>();
                                        if let Some(selection) = tab.rendered_block(
                                            ui,
                                            &mut self.preview_cache,
                                            &r,
                                            &refs,
                                            self.text_size,
                                        ) {
                                            self.jump = Some(selection);
                                        }
                                    }
                                    return;
                                }
                            }
                            let caret = target.as_ref().unwrap_or(&tab.selection).end - offset;
                            let mut layouter = |ui: &egui::Ui, s: &str, w: f32| {
                                ui.fonts(|f| {
                                    f.layout_job(window::layout(
                                        s,
                                        w,
                                        if visual && window::standalone_rule(s) {
                                            egui::Color32::TRANSPARENT
                                        } else {
                                            ui.visuals().text_color()
                                        },
                                        tab.source,
                                        caret,
                                        self.text_size,
                                    ))
                                })
                            };

                            let has_focus = ui.memory(|m| m.has_focus(id));
                            if visual
                                && has_focus
                                && first == last
                                && ui.input(|i| {
                                    i.modifiers.matches_logically(Modifiers::COMMAND)
                                        && i.key_pressed(Key::Enter)
                                })
                                && let Some(pos) =
                                    document::exit_fenced_code(&mut tab.doc.text, active.clone())
                            {
                                ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::Enter));
                                self.jump = Some(pos..pos);
                                tab.doc.refresh_dirty();
                                tab.edited = Some(Instant::now());
                                ctx.request_repaint();
                                return;
                            }
                            if visual
                                && has_focus
                                && ui.input_mut(|i| i.consume_key(Modifiers::COMMAND, Key::A))
                            {
                                self.jump = Some(0..tab.doc.text.chars().count());
                                ctx.request_repaint();
                            }
                            if visual && has_focus && tab.selection.is_empty() {
                                let here = byte_at(&tab.doc.text, tab.selection.start);
                                let join = if first > 0
                                    && here == active.start
                                    && ui.input(|i| {
                                        i.modifiers == Modifiers::NONE
                                            && i.key_pressed(Key::Backspace)
                                    }) {
                                    blocks::paragraph_separator(
                                        &tab.doc.text,
                                        &ranges[first - 1],
                                        &active,
                                    )
                                    .map(|range| (Key::Backspace, range))
                                } else if last + 1 < ranges.len()
                                    && ui.input(|i| {
                                        i.modifiers == Modifiers::NONE && i.key_pressed(Key::Delete)
                                    })
                                {
                                    blocks::paragraph_separator(
                                        &tab.doc.text,
                                        &active,
                                        &ranges[last + 1],
                                    )
                                    .filter(|cut| cut.start == here)
                                    .map(|range| (Key::Delete, range))
                                } else {
                                    None
                                };
                                if let Some((key, cut)) = join {
                                    ui.input_mut(|i| i.consume_key(Modifiers::NONE, key));
                                    let pos = tab.doc.text[..cut.start].chars().count();
                                    tab.doc.text.replace_range(cut, "");
                                    tab.selection = pos..pos;
                                    tab.doc.dirty = true;
                                    tab.edited = Some(Instant::now());
                                    self.jump = Some(tab.selection.clone());
                                    ctx.request_repaint();
                                    return;
                                }
                                if first > 0
                                    && here == active.start
                                    && ui.input_mut(|i| {
                                        i.consume_key(Modifiers::NONE, Key::ArrowLeft)
                                    })
                                {
                                    let next = tab.selection.start - 1;
                                    self.jump = Some(next..next);
                                } else if last + 1 < ranges.len()
                                    && here >= editable.end.min(active.end.saturating_sub(1))
                                    && ui.input_mut(|i| {
                                        i.consume_key(Modifiers::NONE, Key::ArrowRight)
                                    })
                                {
                                    let next = tab.doc.text[..active.end].chars().count();
                                    self.jump = Some(next..next);
                                }
                            }
                            let mut choose = false;
                            let mut moved = false;
                            let mut escape = false;
                            if has_focus
                                && !tab.source
                                && let Some(r) = slash::query(&tab.doc.text, &tab.selection)
                            {
                                let query = tab.doc.text[r.start + 1..r.end].to_lowercase();
                                let count = slash::matching(&query).count();
                                if tab.dismissed.as_ref() != Some(&(r.start, query)) {
                                    ui.input_mut(|i| {
                                        escape = i.consume_key(Modifiers::NONE, Key::Escape);
                                        if count == 0 {
                                            return;
                                        }
                                        if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                                            moved = true;
                                            tab.slash_index = (tab.slash_index + 1) % count;
                                        }
                                        if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                                            moved = true;
                                            tab.slash_index =
                                                (tab.slash_index.min(count - 1) + count - 1)
                                                    % count;
                                        }
                                        choose = i.consume_key(Modifiers::NONE, Key::Enter)
                                            || i.consume_key(Modifiers::NONE, Key::Tab);
                                    });
                                }
                            }
                            if visual
                                && ui.input(|i| i.key_pressed(Key::Escape))
                                && self.find.is_none()
                                && !escape
                            {
                                tab.edit_block = false;
                                ctx.request_repaint();
                            }
                            if visual
                                && has_focus
                                && !choose
                                && tab.selection.is_empty()
                                && ui.input(|i| {
                                    i.key_pressed(Key::Tab)
                                        && (i.modifiers == Modifiers::NONE
                                            || i.modifiers == Modifiers::SHIFT)
                                })
                            {
                                let modifiers = ui.input(|i| i.modifiers);
                                if let Some(pos) = document::nest_item(
                                    &mut tab.doc.text,
                                    tab.selection.start,
                                    modifiers.shift,
                                ) {
                                    ui.input_mut(|i| i.consume_key(modifiers, Key::Tab));
                                    tab.selection = pos..pos;
                                    self.jump = Some(tab.selection.clone());
                                    tab.doc.dirty = true;
                                    tab.edited = Some(Instant::now());
                                    ctx.request_repaint();
                                    return;
                                }
                            }
                            if has_focus
                                && !tab.source
                                && !choose
                                && tab.selection.is_empty()
                                && ui.input(|i| {
                                    i.modifiers == Modifiers::SHIFT && i.key_pressed(Key::Enter)
                                })
                                && let Some(insert) =
                                    document::hard_break(&tab.doc.text, tab.selection.end)
                                && ui.input_mut(|i| i.consume_key(Modifiers::SHIFT, Key::Enter))
                            {
                                ui.input_mut(|i| i.events.push(egui::Event::Paste(insert)));
                            }
                            if has_focus
                                && !tab.source
                                && !choose
                                && tab.selection.is_empty()
                                && ui.input(|i| {
                                    i.key_pressed(Key::Enter) && i.modifiers == Modifiers::NONE
                                })
                                && let Some((range, insert)) =
                                    document::block_enter(&tab.doc.text, tab.selection.end)
                                && ui.input_mut(|i| i.consume_key(Modifiers::NONE, Key::Enter))
                            {
                                let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
                                state.cursor.set_char_range(Some(CCursorRange::two(
                                    CCursor::new(range.start - offset),
                                    CCursor::new(range.end - offset),
                                )));
                                state.store(ctx, id);
                                ui.input_mut(|i| {
                                    i.events.push(if insert.is_empty() {
                                        egui::Event::Key {
                                            key: Key::Backspace,
                                            physical_key: None,
                                            pressed: true,
                                            repeat: false,
                                            modifiers: Modifiers::NONE,
                                        }
                                    } else {
                                        // TextEdit ignores standalone newlines in text events.
                                        egui::Event::Paste(insert)
                                    })
                                });
                            }
                            let vertical =
                                if visual && has_focus && !moved && tab.selection.is_empty() {
                                    ui.input(|i| {
                                        if i.modifiers != Modifiers::NONE {
                                            None
                                        } else if first > 0 && i.key_pressed(Key::ArrowUp) {
                                            Some(true)
                                        } else if last + 1 < ranges.len()
                                            && i.key_pressed(Key::ArrowDown)
                                        {
                                            Some(false)
                                        } else {
                                            None
                                        }
                                    })
                                } else {
                                    None
                                };
                            let mut contents = tab.doc.text[editable.clone()].to_owned();
                            let hint = if visual && tab.doc.text.is_empty() {
                                "Start writing, or type / for commands"
                            } else {
                                ""
                            };
                            if visual && has_focus && tab.selection.is_empty() {
                                let at = tab.selection.start.saturating_sub(offset);
                                let pair = ui.input(|i| {
                                    if i.modifiers != Modifiers::NONE {
                                        return None;
                                    }
                                    if i.key_pressed(Key::Backspace) {
                                        at.checked_sub(2)
                                    } else if i.key_pressed(Key::Delete) {
                                        Some(at)
                                    } else {
                                        None
                                    }
                                });
                                if let Some(pair) = pair {
                                    let byte = byte_at(&contents, pair);
                                    if contents[byte..].starts_with('\\')
                                        && contents
                                            .as_bytes()
                                            .get(byte + 1)
                                            .is_some_and(u8::is_ascii_punctuation)
                                        && layouter(ui, &contents, ui.available_width())
                                            .text()
                                            .chars()
                                            .nth(pair)
                                            == Some('\u{200b}')
                                    {
                                        let mut state =
                                            TextEdit::load_state(ctx, id).unwrap_or_default();
                                        state.cursor.set_char_range(Some(CCursorRange::two(
                                            CCursor::new(pair),
                                            CCursor::new(pair + 2),
                                        )));
                                        state.store(ctx, id);
                                    }
                                }
                            }
                            let output = TextEdit::multiline(&mut contents)
                                .hint_text(hint)
                                .font(egui::FontId::proportional(self.text_size))
                                .id(id)
                                .frame(false)
                                .desired_width(f32::INFINITY)
                                .desired_rows(if visual { 1 } else { 24 })
                                .layouter(&mut layouter)
                                .show(ui);
                            if visual && window::standalone_rule(&contents) {
                                let y = output.galley_pos.y + output.galley.rows[0].rect.center().y;
                                ui.painter().hline(
                                    output.response.rect.x_range(),
                                    y,
                                    ui.visuals().widgets.noninteractive.bg_stroke,
                                );
                            }
                            let (pressed, released, pointer) = ui.input(|i| {
                                (
                                    i.pointer.primary_pressed(),
                                    i.pointer.primary_released(),
                                    i.pointer.interact_pos(),
                                )
                            });
                            if visual
                                && (pressed || released)
                                && let Some(pos) = pointer
                                && output.response.rect.intersect(ui.clip_rect()).contains(pos)
                            {
                                let cursor = offset
                                    + output
                                        .galley
                                        .cursor_from_pos(pos - output.galley_pos)
                                        .ccursor
                                        .index;
                                if pressed && output.response.contains_pointer() {
                                    tab.drag_anchor = Some(cursor);
                                } else if !pressed
                                    && let Some(anchor) = tab.drag_anchor.take()
                                    && (anchor < offset
                                        || anchor > offset + contents.chars().count())
                                {
                                    self.jump = Some(anchor.min(cursor)..anchor.max(cursor));
                                    ctx.request_repaint();
                                }
                            }
                            if let Some(up) = vertical
                                && !output.response.changed()
                            {
                                let cursor =
                                    output.galley.from_ccursor(CCursor::new(caret)).rcursor;
                                let last_content =
                                    contents.trim_end_matches(['\r', '\n']).chars().count();
                                let bottom = output
                                    .galley
                                    .from_ccursor(CCursor::new(last_content))
                                    .rcursor
                                    .row;
                                if (up && cursor.row == 0) || (!up && cursor.row >= bottom) {
                                    let neighbor = if up { first - 1 } else { last + 1 };
                                    let column = if cursor.row == 0 {
                                        cursor.column.saturating_sub(folio::block_prefix(&contents))
                                    } else {
                                        cursor.column
                                    };
                                    let pos = blocks::adjacent_caret(
                                        &tab.doc.text,
                                        ranges[neighbor].clone(),
                                        up,
                                        column,
                                    );
                                    self.jump = Some(pos..pos);
                                    ctx.request_repaint();
                                }
                            }
                            let delta = contents.len() as isize - editable.len() as isize;
                            if output.response.changed() {
                                tab.doc.text.replace_range(editable.clone(), &contents);
                            }
                            for old in &ranges[last + 1..] {
                                let r = old.start.saturating_add_signed(delta)
                                    ..old.end.saturating_add_signed(delta);
                                // Edits can move reference definitions beyond the active block.
                                let refs: Vec<_> = definitions
                                    .iter()
                                    .filter(|d| !active.contains(&d.start))
                                    .map(|d| {
                                        if d.start >= active.end {
                                            d.start.saturating_add_signed(delta)
                                                ..d.end.saturating_add_signed(delta)
                                        } else {
                                            d.clone()
                                        }
                                    })
                                    .collect();
                                if let Some(selection) = tab.rendered_block(
                                    ui,
                                    &mut self.preview_cache,
                                    &r,
                                    &refs,
                                    self.text_size,
                                ) {
                                    self.jump = Some(selection);
                                }
                            }
                            if let Some(cursor) = output.cursor_range {
                                let r = cursor.as_sorted_char_range();
                                tab.selection = (offset + r.start)..(offset + r.end);
                                if visual
                                    && has_focus
                                    && r.is_empty()
                                    && r.start > 0
                                    && let Some(left) = ui.input(|i| {
                                        if i.modifiers != Modifiers::NONE {
                                            return None;
                                        }
                                        if i.key_pressed(Key::ArrowLeft) {
                                            Some(true)
                                        } else if i.key_pressed(Key::ArrowRight) {
                                            Some(false)
                                        } else {
                                            None
                                        }
                                    })
                                {
                                    let byte = byte_at(&contents, r.start);
                                    if contents[..byte].ends_with('\\')
                                        && contents
                                            .as_bytes()
                                            .get(byte)
                                            .is_some_and(u8::is_ascii_punctuation)
                                        && output.galley.text().chars().nth(r.start - 1)
                                            == Some('\u{200b}')
                                    {
                                        let pos = if left {
                                            tab.selection.start - 1
                                        } else {
                                            tab.selection.start + 1
                                        };
                                        tab.selection = pos..pos;
                                        self.jump = Some(tab.selection.clone());
                                        ctx.request_repaint();
                                    }
                                }
                                if visual && output.response.changed() {
                                    let pos = byte_at(&tab.doc.text, tab.selection.end);
                                    let new_blocks = blocks::ranges(&tab.doc.text);
                                    let n = new_blocks
                                        .partition_point(|r| r.end <= pos)
                                        .min(new_blocks.len() - 1);
                                    if new_blocks[n].start != active.start {
                                        self.jump = Some(tab.selection.clone());
                                    }
                                }
                            }
                            if visual
                                && !tab.selection.is_empty()
                                && self.find.is_none()
                                && self.link.is_none()
                                && let Some(cursor) = output.cursor_range
                            {
                                let start = output
                                    .galley
                                    .pos_from_cursor(&cursor.primary)
                                    .translate(output.galley_pos.to_vec2());
                                if ui.clip_rect().intersects(start)
                                    && !ctx.input(|i| i.pointer.is_decidedly_dragging())
                                {
                                    let pos = egui::pos2(
                                        start.left().clamp(
                                            ui.clip_rect().left(),
                                            (ui.clip_rect().right() - 150.)
                                                .max(ui.clip_rect().left()),
                                        ),
                                        (start.top() - 38.).max(ui.clip_rect().top()),
                                    );
                                    egui::Area::new(id.with("selection tools"))
                                        .order(egui::Order::Foreground)
                                        .fixed_pos(pos)
                                        .show(ctx, |ui| {
                                            egui::Frame::popup(ui.style()).inner_margin(5.).show(
                                                ui,
                                                |ui| {
                                                    ui.horizontal(|ui| {
                                                        for (caption, label, cmd) in [
                                                            (
                                                                egui::RichText::new("B").strong(),
                                                                "Bold",
                                                                Some(Format::Wrap("**")),
                                                            ),
                                                            (
                                                                egui::RichText::new("I").italics(),
                                                                "Italic",
                                                                Some(Format::Wrap("*")),
                                                            ),
                                                            (
                                                                egui::RichText::new("</>")
                                                                    .monospace(),
                                                                "Inline code",
                                                                Some(Format::Wrap("`")),
                                                            ),
                                                            (
                                                                egui::RichText::new("Link"),
                                                                "Insert link",
                                                                None,
                                                            ),
                                                        ] {
                                                            if ui
                                                                .button(caption)
                                                                .on_hover_text(label)
                                                                .clicked()
                                                            {
                                                                bubble_action = Some(cmd);
                                                            }
                                                        }
                                                    });
                                                },
                                            );
                                        });
                                }
                            }
                            if let Some(target) = target {
                                let c = output
                                    .galley
                                    .from_ccursor(CCursor::new(target.start - offset));
                                ui.scroll_to_rect(
                                    output
                                        .galley
                                        .pos_from_cursor(&c)
                                        .translate(output.galley_pos.to_vec2()),
                                    None,
                                );
                            }
                            if output.response.changed() {
                                tab.dismissed = None;
                                tab.slash_index = 0;
                            }
                            if let Some(r) =
                                slash::query(&tab.doc.text, &tab.selection).filter(|_| {
                                    !tab.source && (has_focus || output.response.has_focus())
                                })
                            {
                                let query = tab.doc.text[r.start + 1..r.end].to_lowercase();
                                if escape {
                                    tab.dismissed = Some((r.start, query.clone()));
                                }
                                if tab.dismissed.as_ref() != Some(&(r.start, query.clone())) {
                                    let matches: Vec<_> = slash::matching(&query).collect();

                                    ui.memory_mut(|m| {
                                        m.set_focus_lock_filter(
                                            id,
                                            egui::EventFilter {
                                                escape: true,
                                                tab: !matches.is_empty(),
                                                horizontal_arrows: true,
                                                vertical_arrows: true,
                                            },
                                        )
                                    });
                                    tab.slash_index =
                                        tab.slash_index.min(matches.len().saturating_sub(1));
                                    let caret = output
                                        .galley
                                        .from_ccursor(CCursor::new(tab.selection.end - offset));
                                    let rect = output
                                        .galley
                                        .pos_from_cursor(&caret)
                                        .translate(output.galley_pos.to_vec2());
                                    let above = ctx.screen_rect().bottom() - rect.bottom() < 230.
                                        && rect.top() > 230.;
                                    let mut picked =
                                        (choose && !matches.is_empty()).then_some(tab.slash_index);
                                    egui::Area::new(id.with("slash"))
                                        .order(egui::Order::Foreground)
                                        .default_size(egui::vec2(248., 300.))
                                        .pivot(if above {
                                            egui::Align2::LEFT_BOTTOM
                                        } else {
                                            egui::Align2::LEFT_TOP
                                        })
                                        .fixed_pos(if above {
                                            rect.left_top()
                                        } else {
                                            rect.left_bottom()
                                        })
                                        .show(ctx, |ui| {
                                            egui::Frame::popup(ui.style())
                                                .corner_radius(10)
                                                .inner_margin(8)
                                                .show(ui, |ui| {
                                                    ui.set_width(232.);
                                                    if matches.is_empty() {
                                                        ui.weak("No matching commands");
                                                        return;
                                                    }
                                                    ui.spacing_mut().interact_size.y = 28.;
                                                    ui.spacing_mut().item_spacing.y = 2.;
                                                    egui::ScrollArea::vertical()
                                                        .max_height(258.)
                                                        .show(ui, |ui| {
                                                            for (i, c) in matches.iter().enumerate()
                                                            {
                                                                let row = ui.add_sized(
                                                                    [ui.available_width(), 28.],
                                                                    egui::SelectableLabel::new(
                                                                        i == tab.slash_index,
                                                                        c.name,
                                                                    ),
                                                                );
                                                                if moved && i == tab.slash_index {
                                                                    row.scroll_to_me(None);
                                                                }
                                                                if row.clicked() {
                                                                    picked = Some(i);
                                                                }
                                                            }
                                                        });
                                                });
                                        });
                                    if let Some(i) = picked {
                                        if matches!(matches[i].name, "Link" | "Image") {
                                            self.link = Some(link::Link::new(
                                                tab.id,
                                                r,
                                                String::new(),
                                                matches[i].name == "Image",
                                            ));
                                            ctx.request_repaint();
                                            return;
                                        }
                                        let pos = slash::insert(&mut tab.doc.text, r, matches[i]);
                                        self.jump = Some(pos..pos);
                                        tab.selection = pos..pos;
                                        tab.doc.dirty = true;
                                        tab.edited = Some(Instant::now());
                                        ctx.request_repaint();
                                    }
                                }
                            }

                            if output.response.changed() {
                                tab.doc.dirty = true;
                                tab.edited = Some(Instant::now());
                            }
                        });
                    },
                );
            });
        if let Some(command) = bubble_action {
            if let Some(format) = command {
                self.apply(format);
            } else {
                self.insert_link(false);
            }
        }
    }
}

impl eframe::App for Editor {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        storage.set_string("text_size", self.text_size.to_string());
        storage.set_string("page_width", self.line_width.to_string());
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, input: &mut egui::RawInput) {
        if self.link.is_some()
            || self.error.is_some()
            || self.closing.is_some()
            || ctx.memory(|m| m.has_focus(egui::Id::new("find query")))
        {
            return;
        }
        input.events.retain(|event| {
            let egui::Event::Paste(text) = event else {
                return true;
            };
            match attachment::clipboard() {
                Ok(images) if !images.is_empty() => {
                    self.insert_images(images);
                    false
                }
                Err(e) if text.is_empty() => {
                    self.error = Some(format!("Could not read clipboard. {e}"));
                    false
                }
                _ => !text.is_empty() && !self.paste_link(text),
            }
        });
    }
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        #[cfg(target_os = "macos")]
        for path in mac_open::pending() {
            self.open(&path);
        }
        let shortcuts_enabled =
            self.closing.is_none() && self.error.is_none() && self.link.is_none();
        if shortcuts_enabled {
            self.shortcuts(ctx);
            self.handle_drops(ctx);
        }
        self.toolbar(ctx);
        self.editor(ctx);
        let tab = &mut self.tabs[self.active];
        if let Some((range, updated)) = tab.table_edit.take() {
            let caret = tab.doc.text[..range.start].chars().count();
            tab.doc.text.replace_range(range, &updated);
            tab.selection = caret..caret;
            tab.edit_block = false;
            tab.doc.dirty = true;
            tab.edited = Some(Instant::now());
            self.jump = None;
            ctx.request_repaint();
        }
        if let Some(link) = &mut self.link
            && let Some(result) = link.show(ctx)
        {
            if let Some(markdown) = result
                && self.tabs[self.active].id == link.tab
                && self.tabs[self.active]
                    .doc
                    .text
                    .get(link.range.clone())
                    .is_some()
            {
                let tab = &mut self.tabs[self.active];
                let caret =
                    tab.doc.text[..link.range.start].chars().count() + markdown.chars().count();
                tab.doc.text.replace_range(link.range.clone(), &markdown);
                tab.doc.dirty = true;
                tab.edited = Some(Instant::now());
                self.jump = Some(caret..caret);
            }
            self.link = None;
            ctx.request_repaint();
        }
        for tab in &mut self.tabs {
            if tab.edited != tab.recorded {
                tab.history.record(&tab.doc.text);
                tab.recorded = tab.edited;
            }
        }
        if shortcuts_enabled
            && self.closing.is_none()
            && self.error.is_none()
            && self.link.is_none()
        {
            self.finish_document_shortcuts(ctx);
        }
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quit
            && self.tabs.iter().any(|t| t.doc.dirty)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.finish_close(ctx, Closing::App);
        }
        if self.closing.is_none() {
            for tab in &mut self.tabs {
                if let (Some(time), Some(path)) = (tab.edited, tab.doc.path.clone()) {
                    if time.elapsed() >= Duration::from_secs(2) {
                        tab.edited = None;
                        if let Err(e) = tab.doc.save(&path) {
                            self.error =
                                Some(format!("Autosave failed for {}. {e}", path.display()));
                        }
                    } else {
                        ctx.request_repaint_after(
                            Duration::from_secs(2) - time.elapsed().min(Duration::from_secs(2)),
                        );
                    }
                }
            }
        }
        let tab = &self.tabs[self.active];
        let title = format!(
            "{}{} — Folio",
            tab.doc.name(),
            if tab.doc.dirty { " *" } else { "" }
        );
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }
        if let Some(message) = self.error.clone() {
            egui::Modal::new("error".into()).show(ctx, |ui| {
                ui.label(message);
                if ui.button("OK").clicked() {
                    self.error = None;
                }
            });
        } else if let Some(kind) = self.closing {
            egui::Modal::new("close".into()).show(ctx, |ui| {
                ui.heading(format!(
                    "Save changes to {}?",
                    self.tabs[self.active].doc.name()
                ));
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() && self.save(false) {
                        self.finish_close(ctx, kind);
                    }
                    if ui.button("Discard").clicked() {
                        if matches!(kind, Closing::App) {
                            self.close_tab();
                        }
                        self.finish_close(ctx, kind);
                    }
                    if ui.button("Cancel").clicked() {
                        self.closing = None;
                    }
                });
            });
        }
    }
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "Ubuntu Bold".into(),
        egui::FontData::from_static(include_bytes!("../assets/fonts/Ubuntu-Bold.ttf")).into(),
    );
    let mut family = fonts.families[&egui::FontFamily::Proportional].clone();
    family.insert(0, "Ubuntu Bold".into());
    fonts
        .families
        .insert(egui::FontFamily::Name("Bold".into()), family);
    ctx.set_fonts(fonts);
    ctx.style_mut(|style| {
        let dark = style.visuals.dark_mode;
        let v = &mut style.visuals;
        if dark {
            v.panel_fill = egui::Color32::from_rgb(28, 32, 39);
            v.window_fill = egui::Color32::from_rgb(39, 45, 55);
            v.override_text_color = Some(egui::Color32::from_rgb(231, 233, 225));
            v.selection.bg_fill = egui::Color32::from_rgb(49, 89, 84);
        }
        style.spacing.item_spacing = egui::vec2(7., 5.);
    });
}

fn self_test() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Field notes.md");
    std::fs::write(&path, "\u{feff}# 🦀 Café\r\n").unwrap();
    let mut doc = Document::open(&path).unwrap();
    document::format(&mut doc.text, 4..8, Format::Wrap("**"));
    doc.save(&path).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "\u{feff}# 🦀 **Café**\r\n"
    );
    let ctx = egui::Context::default();
    configure_fonts(&ctx);
    for source in [false, true] {
        let output = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let mut layout = |ui: &egui::Ui, s: &str, w: f32| {
                    ui.fonts(|f| {
                        f.layout_job(window::layout(s, w, ui.visuals().text_color(), source, 0,17.))
                    })
                };
                let field = TextEdit::multiline(&mut doc.text)
                    .layouter(&mut layout)
                    .show(ui);
                if source { assert_eq!(field.galley.text(), doc.text); } else { assert_eq!(field.galley.text().replace('\u{200b}', ""), "🦀 Café\n"); }
                if !source {
                    window::preview(ui,&mut Default::default(),"Heading\n=======\n\n| A | B |\n|---|---|\n| **one** | ~~two~~ |\n\n> quote\n\n- [x] done\n\nA note[^1]\n\n[^1]: footnote",None,17.);
                }
            });
        });
        assert!(!output.shapes.is_empty());
    }
}
fn main() -> eframe::Result {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--self-test")) {
        self_test();
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    mac_open::on_launch();
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([940., 660.])
            .with_min_inner_size([480., 320.]),
        ..Default::default()
    };
    eframe::run_native(
        "Folio",
        native,
        Box::new(|cc| {
            #[cfg(target_os = "macos")]
            {
                mac_open::install()?;
                mac_open::attach(&cc.egui_ctx);
            }
            configure_fonts(&cc.egui_ctx);
            let mut editor = Editor::new();
            if let Some(width) = cc
                .storage
                .and_then(|s| s.get_string("page_width"))
                .and_then(|s| s.parse::<f32>().ok())
                .filter(|w| w.is_finite())
            {
                editor.line_width = width.clamp(480., 1050.);
            }
            if let Some(size) = cc
                .storage
                .and_then(|s| s.get_string("text_size"))
                .and_then(|s| s.parse::<f32>().ok())
                .filter(|x| x.is_finite())
            {
                editor.text_size = size.clamp(13., 26.);
            }
            Ok(Box::new(editor))
        }),
    )
}

#[cfg(test)]
mod drop_tests {
    use super::*;
    #[test]
    fn find_ignores_case_and_wraps_with_original_character_offsets() {
        let mut app = Editor::with_documents([]);
        app.tabs[0].doc.text = "🦀 Café / CAFÉ".into();
        app.find = Some("café".into());
        for expected in [2..6, 9..13, 2..6] {
            app.search();
            assert_eq!(app.jump, Some(expected.clone()));
            app.tabs[0].selection = expected;
        }
    }

    #[test]
    fn launch_documents_precede_the_untitled_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Field journal.md");
        std::fs::write(&path, "# Journal").unwrap();
        let app = Editor::with_documents([path.clone(), path.clone()]);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.tabs[0].doc.text, "# Journal");
        let mut app = Editor::with_documents([]);
        app.tabs[0].doc.text.push_str("draft");
        app.open(&path);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.tabs[0].doc.text, "draft");
    }

    #[test]
    fn dropped_image_replaces_selection_and_markdown_opens_as_a_tab() {
        let dir = tempfile::tempdir().unwrap();
        let note = dir.path().join("note.md");
        let other = dir.path().join("another.md");
        let image = dir.path().join("tiny.png");
        std::fs::write(&note, "a 🦀 b").unwrap();
        std::fs::write(&other, "# Other").unwrap();
        image::RgbaImage::new(1, 1).save(&image).unwrap();
        let mut editor = Editor::new();
        editor.open(&note);
        let original = editor.active;
        editor.tabs[original].selection = 2..3;
        let raw = egui::RawInput {
            dropped_files: [image, other]
                .into_iter()
                .map(|p| egui::DroppedFile {
                    path: Some(p),
                    ..Default::default()
                })
                .collect(),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let _ = ctx.run(raw, |ctx| editor.handle_drops(ctx));
        assert_eq!(editor.tabs[original].doc.text, "a ![tiny](tiny.png) b");
        assert_eq!(editor.tabs[editor.active].doc.text, "# Other");
    }
}

#[cfg(test)]
mod slash_menu_tests {
    use super::*;

    #[test]
    fn switching_to_source_accepts_typing_at_the_visual_selection() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([]);
        let mut frame = eframe::Frame::_new_kittest();
        for events in [
            vec![],
            vec![egui::Event::Text("café".into())],
            vec![egui::Event::Key {
                key: Key::Slash,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            }],
            vec![egui::Event::Text("!".into())],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut app, ctx, &mut frame),
            );
        }
        assert!(app.tabs[0].source);
        assert_eq!(app.tabs[0].doc.text, "café!");
    }

    #[test]
    fn toolbar_divider_keeps_the_selection_and_following_paragraph_text() {
        let mut app = Editor::with_documents([]);
        app.tabs[0].doc.text = "First paragraph\nNext".into();
        app.tabs[0].selection = 0..5;
        app.insert_block(&slash::DIVIDER);
        let caret = app.jump.unwrap().start;
        let doc = &mut app.tabs[0].doc;
        doc.text.insert_str(byte_at(&doc.text, caret), "Below");
        assert_eq!(doc.text, "First paragraph\n\n***\nBelow\n\nNext");
        assert!(doc.dirty);
    }

    #[test]
    fn blank_write_view_shows_the_slash_hint_until_typing() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([]);
        let mut frame = eframe::Frame::_new_kittest();
        for (events, visible) in [
            (vec![], true),
            (vec![egui::Event::Text("Note".into())], false),
        ] {
            let output = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut app, ctx, &mut frame),
            );
            assert_eq!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("type / for commands"))), visible);
        }
        assert_eq!(app.tabs[0].doc.text, "Note");
    }

    #[test]
    fn enter_on_an_empty_quote_starts_an_outside_paragraph() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([]);
        let mut frame = eframe::Frame::_new_kittest();
        let enter = egui::Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        for events in [
            vec![],
            vec![egui::Event::Text("> First".into())],
            vec![enter.clone()],
            vec![egui::Event::Text("Second".into())],
            vec![enter.clone()],
            vec![enter],
            vec![egui::Event::Text("Outside".into())],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut app, ctx, &mut frame),
            );
        }
        assert_eq!(app.tabs[0].doc.text, "> First\n> Second\n\nOutside");
    }

    #[test]
    fn formatting_a_selection_inside_a_table_cell_round_trips_to_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Table.md");
        let original = "| Name | Status |\n| --- | --- |\n| Alpine | Prêt |\n";
        std::fs::write(&path, original).unwrap();
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([path.clone()]);
        let mut frame = eframe::Frame::_new_kittest();
        let mut run = |events| {
            ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut app, ctx, &mut frame),
            )
        };
        let ends = |output: &egui::FullOutput| {
            output
                .shapes
                .iter()
                .find_map(|entry| {
                    let egui::Shape::Text(shape) = &entry.shape else {
                        return None;
                    };
                    let galley = &shape.galley;
                    if galley.text() != "Prêt" {
                        return None;
                    }
                    let pos = |n| {
                        shape.pos
                            + galley
                                .pos_from_cursor(&galley.from_ccursor(CCursor::new(n)))
                                .center()
                                .to_vec2()
                    };
                    Some((pos(0) + egui::vec2(0.5, 0.), pos(4)))
                })
                .unwrap()
        };
        let button = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let (pos, _) = ends(&run(vec![]));
        run(vec![egui::Event::PointerMoved(pos), button(pos, true)]);
        run(vec![button(pos, false)]);
        let (start, end) = ends(&run(vec![]));
        run(vec![egui::Event::PointerMoved(start), button(start, true)]);
        run(vec![egui::Event::PointerMoved(end)]);
        run(vec![button(end, false)]);
        for key in [Key::B, Key::S] {
            run(vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::COMMAND,
            }]);
        }
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            original.replace("Prêt", "**Prêt**")
        );
    }

    #[test]
    fn dragging_between_paragraphs_selects_the_source_span() {
        for (active, reverse) in [(false, false), (true, false), (false, true), (true, true)] {
            let ctx = egui::Context::default();
            configure_fonts(&ctx);
            let mut app = Editor::with_documents([]);
            let mut doc = Document::default();
            doc.text = "café\n\nsecond".into();
            app.add(doc);
            if active {
                app.jump = Some(0..0);
            }
            let mut frame = eframe::Frame::_new_kittest();
            let mut run = |events| {
                ctx.run(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ctx| eframe::App::update(&mut app, ctx, &mut frame),
                )
            };
            let output = run(vec![]);
            let position = |word: &str, n| {
                output
                    .shapes
                    .iter()
                    .find_map(|entry| {
                        let egui::Shape::Text(t) = &entry.shape else {
                            return None;
                        };
                        if t.galley.text() != word {
                            return None;
                        }
                        let cursor = t.galley.from_ccursor(CCursor::new(n));
                        Some(t.pos + t.galley.pos_from_cursor(&cursor).center().to_vec2())
                    })
                    .unwrap()
            };
            let start = position("café", 1);
            let end = position("second", 3);
            let (start, end) = if reverse { (end, start) } else { (start, end) };
            let button = |pos, pressed| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            run(vec![egui::Event::PointerMoved(start), button(start, true)]);
            run(vec![egui::Event::PointerMoved(end)]);
            run(vec![button(end, false)]);
            run(vec![]);
            run(vec![egui::Event::Text("X".into())]);
            assert_eq!(app.tabs[app.active].doc.text, "cXond", "active={active}");
        }
    }

    #[test]
    fn formatting_hints_follow_the_platform_and_registered_shortcuts() {
        let ctx = egui::Context::default();
        let _ = ctx.run(Default::default(), |_| {});
        for os in [
            egui::os::OperatingSystem::Mac,
            egui::os::OperatingSystem::Windows,
            egui::os::OperatingSystem::Nix,
        ] {
            ctx.set_os(os);
            assert_eq!(
                shortcut_label(&ctx, Format::Block("### ")),
                Some(
                    ctx.format_shortcut(&egui::KeyboardShortcut::new(
                        Modifiers::COMMAND,
                        Key::Num3
                    ))
                )
            );
            assert!(shortcut_label(&ctx, Format::Block("> ")).is_none());
        }
    }

    #[test]
    fn unchanged_title_does_not_schedule_another_frame() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([]);
        let mut frame = eframe::Frame::_new_kittest();
        for (events, expected) in [
            (vec![], true),
            (vec![], false),
            (vec![egui::Event::Text("edited".into())], true),
            (vec![], false),
        ] {
            let output = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut app, ctx, &mut frame),
            );
            assert_eq!(
                output.viewport_output[&egui::ViewportId::ROOT]
                    .commands
                    .iter()
                    .any(|cmd| matches!(cmd, egui::ViewportCommand::Title(_))),
                expected
            );
        }
    }

    #[test]
    fn reopening_an_empty_document_accepts_a_footnote_and_saves_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Note.md");
        std::fs::write(&path, "").unwrap();
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([path.clone()]);
        app.open(&path);
        let mut frame = eframe::Frame::_new_kittest();
        let mut run = |events, modifiers| {
            let input = egui::RawInput {
                events,
                modifiers,
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut frame));
        };
        let key = |key, modifiers| egui::Event::Key {
            key,
            modifiers,
            pressed: true,
            repeat: false,
            physical_key: None,
        };
        run(vec![], Modifiers::NONE);
        run(vec![egui::Event::Text("/footnote".into())], Modifiers::NONE);
        run(vec![key(Key::Enter, Modifiers::NONE)], Modifiers::NONE);
        run(vec![], Modifiers::NONE);
        run(vec![egui::Event::Text("Checked".into())], Modifiers::NONE);
        run(vec![key(Key::S, Modifiers::COMMAND)], Modifiers::COMMAND);
        assert_eq!(
            std::fs::read_to_string(path).unwrap().trim_end(),
            "[^1]\n\n[^1]: Checked"
        );
    }

    #[test]
    fn saved_list_conversion_can_be_undone_and_redone() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Undo.md");
        std::fs::write(&path, "Trail notes").unwrap();
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([path.clone()]);
        let mut frame = eframe::Frame::_new_kittest();
        let mut press = |key, modifiers| {
            let input = egui::RawInput {
                modifiers,
                events: vec![egui::Event::Key {
                    key,
                    modifiers,
                    pressed: true,
                    repeat: false,
                    physical_key: None,
                }],
                ..Default::default()
            };
            let _ = ctx.run(input, |ctx| eframe::App::update(&mut app, ctx, &mut frame));
        };
        let cmd = Modifiers::COMMAND;
        for (key, modifiers, expected) in [
            (Key::Num8, cmd | Modifiers::SHIFT, "- Trail notes"),
            (Key::Z, cmd, "Trail notes"),
            (Key::Z, cmd | Modifiers::SHIFT, "- Trail notes"),
        ] {
            press(key, modifiers);
            press(Key::S, cmd);
            assert_eq!(std::fs::read_to_string(&path).unwrap(), expected);
        }
    }

    #[test]
    fn new_tab_keeps_pending_text_in_the_original_document() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([]);
        let mut frame = eframe::Frame::_new_kittest();
        for events in [
            vec![],
            vec![
                egui::Event::Text("Finish this note".into()),
                egui::Event::Key {
                    key: Key::N,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::COMMAND,
                },
            ],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| eframe::App::update(&mut app, ctx, &mut frame),
            );
        }
        assert_eq!(app.tabs[0].doc.text, "Finish this note");
        assert_eq!(app.active, 1);
        assert!(app.tabs[1].doc.text.is_empty());
    }

    #[test]
    fn native_close_protects_an_edit_received_in_the_same_frame() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([]);
        let mut frame = eframe::Frame::_new_kittest();
        let update = |app: &mut Editor, frame: &mut eframe::Frame, input| {
            ctx.run(input, |ctx| eframe::App::update(app, ctx, frame))
        };
        let _ = update(&mut app, &mut frame, egui::RawInput::default());
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Text("Last edit".into()));
        input
            .viewports
            .get_mut(&egui::ViewportId::ROOT)
            .unwrap()
            .events
            .push(egui::ViewportEvent::Close);
        let output = update(&mut app, &mut frame, input);
        assert_eq!(app.tabs[0].doc.text, "Last edit");
        assert!(matches!(app.closing, Some(Closing::App)));
        assert!(
            output.viewport_output[&egui::ViewportId::ROOT]
                .commands
                .iter()
                .any(|c| matches!(c, egui::ViewportCommand::CancelClose))
        );
    }

    #[test]
    fn close_shortcut_prompts_for_text_typed_in_the_same_frame() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([]);
        let original = app.tabs[0].id;
        for events in [
            vec![],
            vec![
                egui::Event::Text("Draft".into()),
                egui::Event::Key {
                    key: Key::W,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::COMMAND,
                },
            ],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.shortcuts(ctx);
                    app.editor(ctx);
                    app.finish_document_shortcuts(ctx);
                },
            );
        }
        assert_eq!(app.tabs[0].id, original);
        assert_eq!(app.tabs[0].doc.text, "Draft");
        assert!(matches!(app.closing, Some(Closing::Tab)));
    }

    #[test]
    fn keyboard_authoring_saves_a_heading_and_list_as_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Authoring.md");
        std::fs::write(&path, "").unwrap();
        let mut app = Editor::with_documents([path.clone()]);
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let key = |key, modifiers| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        };
        for events in [
            vec![],
            vec![key(Key::Num1, Modifiers::COMMAND)],
            vec![],
            vec![egui::Event::Text("Field notes".into())],
            vec![key(Key::Enter, Modifiers::NONE)],
            vec![key(Key::Enter, Modifiers::NONE)],
            vec![egui::Event::Text("- First stop".into())],
            vec![key(Key::Enter, Modifiers::NONE)],
            vec![
                egui::Event::Text("Second stop".into()),
                key(Key::S, Modifiers::COMMAND),
            ],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| {
                    app.shortcuts(ctx);
                    app.editor(ctx);
                    app.finish_document_shortcuts(ctx);
                },
            );
        }
        assert_eq!(
            Document::open(&path).unwrap().text,
            "# Field notes\n\n- First stop\n- Second stop"
        );
    }

    #[test]
    fn horizontal_editing_skips_hidden_escapes_but_keeps_code_literal() {
        for (source, arrows, expected) in [
            (r"a\*b", vec![Key::ArrowLeft, Key::Backspace], "axb"),
            (
                r"a\*b",
                vec![Key::ArrowLeft, Key::ArrowLeft, Key::Delete],
                "axb",
            ),
            (r"a\*b", vec![Key::ArrowLeft; 2], r"ax\*b"),
            (
                r"a\*b",
                vec![Key::ArrowLeft, Key::ArrowLeft, Key::ArrowRight],
                r"a\*xb",
            ),
            (r"`a\*b`", vec![Key::ArrowLeft; 3], r"`a\x*b`"),
        ] {
            let ctx = egui::Context::default();
            configure_fonts(&ctx);
            let mut app = Editor::with_documents([]);
            let mut frame = |events| {
                let _ = ctx.run(
                    egui::RawInput {
                        events,
                        ..Default::default()
                    },
                    |ctx| app.editor(ctx),
                );
            };
            frame(vec![]);
            frame(vec![egui::Event::Text(source.into())]);
            for key in arrows {
                frame(vec![egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }]);
                frame(vec![]);
            }
            frame(vec![egui::Event::Text("x".into())]);
            assert_eq!(app.tabs[0].doc.text, expected);
        }
    }

    #[test]
    fn typing_a_nested_list_keeps_the_parent_at_body_size() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::with_documents([]);
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        for event in [
            None,
            Some(egui::Event::Text("- Hey".into())),
            Some(key(Key::Enter)),
            Some(key(Key::Tab)),
            None,
            Some(egui::Event::Text("Howdy".into())),
            Some(key(Key::Enter)),
            Some(key(Key::Tab)),
            None,
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    events: event.into_iter().collect(),
                    ..Default::default()
                },
                |ctx| app.editor(ctx),
            );
        }
        let doc = &app.tabs[app.active].doc.text;
        assert_eq!(doc, "- Hey\n\n  - Howdy\n\n    - ");
        let layout = window::layout(doc, 480., egui::Color32::WHITE, false, 0, 17.);
        let at = layout.text.find("Howdy").unwrap();
        let section = layout
            .sections
            .iter()
            .find(|s| s.byte_range.contains(&at))
            .unwrap();
        assert_eq!(section.format.font_id.size, 17.);
    }

    #[test]
    fn tab_and_shift_tab_change_the_visual_list_nesting() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::new();
        let mut doc = Document::default();
        doc.text = "- parent\n- child".into();
        app.add(doc);
        app.jump = Some(16..16);
        for modifiers in [None, Some(Modifiers::NONE), None, Some(Modifiers::SHIFT)] {
            let mut input = egui::RawInput::default();
            if let Some(m) = modifiers {
                input.modifiers = m;
                input.events.push(egui::Event::Key {
                    key: Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: m,
                });
            }
            let _ = ctx.run(input, |ctx| app.editor(ctx));
            if modifiers == Some(Modifiers::NONE) {
                assert_eq!(app.tabs[app.active].doc.text, "- parent\n  - child");
            }
        }
        assert_eq!(app.tabs[app.active].doc.text, "- parent\n- child");
    }
    #[test]
    fn pasting_a_url_in_write_can_be_undone_and_source_paste_is_literal() {
        let mut app = Editor::new();
        let mut doc = Document::default();
        doc.text = "Meet **at noon**.".into();
        app.add(doc);
        app.tabs[app.active].selection = 7..14;
        assert!(app.paste_link("https://example.org"));
        let tab = &mut app.tabs[app.active];
        assert_eq!(tab.doc.text, "Meet **[at noon](https://example.org)**.");
        tab.history.record(&tab.doc.text);
        tab.history.restore(&mut tab.doc.text, false).unwrap();
        assert_eq!(tab.doc.text, "Meet **at noon**.");
        tab.selection = 7..14;
        tab.source = true;
        assert!(!app.paste_link("https://example.org"));
    }
    #[test]
    fn keyboard_accepts_checkbox_alias_and_escape_preserves_literal_query() {
        let ctx = egui::Context::default();
        configure_fonts(&ctx);
        let mut app = Editor::new();
        app.add(Document::default());
        let key = |key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let frame = |app: &mut Editor, events| {
            let _ = ctx.run(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ctx| app.editor(ctx),
            );
        };
        frame(&mut app, vec![]);
        frame(&mut app, vec![egui::Event::Text("/checkbox".into())]);
        frame(&mut app, vec![key(Key::Enter)]);
        frame(&mut app, vec![egui::Event::Text("Pack map".into())]);
        assert_eq!(app.tabs[app.active].doc.text, "- [ ] Pack map");
        app.add(Document::default());
        frame(&mut app, vec![]);
        frame(&mut app, vec![egui::Event::Text("/grid".into())]);
        frame(&mut app, vec![key(Key::Escape)]);
        assert_eq!(app.tabs[app.active].doc.text, "/grid");
        assert!(app.tabs[app.active].edit_block);
        frame(&mut app, vec![key(Key::Escape)]);
        assert!(!app.tabs[app.active].edit_block);

        app.add(Document::default());
        frame(&mut app, vec![]);
        frame(&mut app, vec![egui::Event::Text("/nomatches".into())]);
        frame(&mut app, vec![key(Key::Escape)]);
        assert!(app.tabs[app.active].edit_block);
        assert_eq!(app.tabs[app.active].doc.text, "/nomatches");
        frame(&mut app, vec![egui::Event::Text(" here".into())]);
        assert_eq!(app.tabs[app.active].doc.text, "/nomatches here");
    }
}
