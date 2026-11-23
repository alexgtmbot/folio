#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
mod document;
mod slash;
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
    source: bool,
    slash_index: usize,
    dismissed: Option<(usize, String)>,
    edited: Option<Instant>,
}
#[derive(Clone, Copy)]
enum Closing {
    Tab,
    App,
}

struct Editor {
    tabs: Vec<Tab>,
    active: usize,
    next_id: u64,
    preview_cache: egui_commonmark::CommonMarkCache,
    closing: Option<Closing>,
    quit: bool,
    error: Option<String>,
    find: Option<String>,
    focus_find: bool,
    jump: Option<Range<usize>>,
}

impl Editor {
    fn new() -> Self {
        let mut app = Self {
            tabs: Vec::new(),
            active: 0,
            next_id: 0,
            preview_cache: Default::default(),
            closing: None,
            quit: false,
            error: None,
            find: None,
            focus_find: false,
            jump: None,
        };
        for p in std::env::args_os().skip(1) {
            app.open(Path::new(&p));
        }
        if app.tabs.is_empty() {
            app.add(Document::default());
        }
        app
    }
    fn add(&mut self, doc: Document) {
        self.tabs.push(Tab {
            id: self.next_id,
            doc,
            source: false,
            slash_index: 0,
            dismissed: None,
            selection: 0..0,
            edited: None,
        });
        self.next_id += 1;
        self.active = self.tabs.len() - 1;
        self.jump = Some(0..0);
    }
    fn open(&mut self, path: &Path) {
        match Document::open(path) {
            Ok(doc) => {
                if let Some(i) = self.tabs.iter().position(|t| t.doc.path == doc.path) {
                    self.active = i;
                    self.jump = None;
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
        if let Some(pos) = text[start..]
            .find(query)
            .map(|i| i + start)
            .or_else(|| text.find(query))
        {
            let n = text[..pos].chars().count();
            self.jump = Some(n..n + query.chars().count());
        } else {
            self.error = Some("No matching text.".into());
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
    fn shortcuts(&mut self, ctx: &egui::Context) {
        let key = |m, k| ctx.input_mut(|i| i.consume_key(m, k));
        let cmd = Modifiers::COMMAND;
        let shifted = cmd | Modifiers::SHIFT;
        if key(shifted, Key::S) {
            self.save(true);
        } else if key(cmd, Key::S) {
            self.save(false);
        }
        if key(cmd, Key::N) {
            self.add(Document::default());
        }
        if key(cmd, Key::O) {
            self.open_dialog();
        }
        if key(cmd, Key::W) {
            if self.tabs[self.active].doc.dirty {
                self.closing = Some(Closing::Tab);
            } else {
                self.close_tab();
            }
        }
        if key(cmd, Key::F) {
            self.find = Some(String::new());
            self.focus_find = true;
        }
        if key(cmd, Key::Slash) {
            let tab = &mut self.tabs[self.active];
            tab.source = !tab.source;
        }
        for (m, k, f) in [
            (cmd, Key::B, Format::Wrap("**")),
            (cmd, Key::I, Format::Wrap("*")),
            (cmd, Key::E, Format::Wrap("`")),
            (cmd, Key::Num0, Format::Block("")),
            (cmd, Key::Num1, Format::Block("# ")),
            (cmd, Key::Num2, Format::Block("## ")),
            (cmd, Key::Num3, Format::Block("### ")),
            (shifted, Key::T, Format::Task),
        ] {
            if key(m, k) {
                self.apply(f);
            }
        }
    }
    fn toolbar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("commands").show(ctx, |ui| {
            ui.horizontal(|ui| {
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
                                    if self.tabs[self.active].doc.dirty {
                                        self.closing = Some(Closing::Tab)
                                    } else {
                                        self.close_tab()
                                    }
                                }
                            }
                        }
                    }
                });
                ui.menu_button("Format", |ui| {
                    for (name, f) in [
                        ("Paragraph", Format::Block("")),
                        ("Heading 1", Format::Block("# ")),
                        ("Heading 2", Format::Block("## ")),
                        ("Heading 3", Format::Block("### ")),
                        ("Bold", Format::Wrap("**")),
                        ("Italic", Format::Wrap("*")),
                        ("Inline code", Format::Wrap("`")),
                        ("Bullet list", Format::Block("- ")),
                        ("Quote", Format::Block("> ")),
                        ("Toggle task", Format::Task),
                    ] {
                        if ui.button(name).clicked() {
                            self.apply(f);
                            ui.close_menu();
                        }
                    }
                });
                if ui.button("Find").clicked() {
                    self.find = Some(String::new());
                    self.focus_find = true;
                }
            });
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
                    let response = ui.text_edit_singleline(self.find.as_mut().unwrap());
                    if self.focus_find {
                        response.request_focus();
                        self.focus_find = false;
                    }
                    if ui.button("Find next").clicked()
                        || response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter))
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
        let tab = &mut self.tabs[self.active];
        let id = egui::Id::new(("editor", tab.id));
        egui::SidePanel::left("outline")
            .default_width(155.)
            .show(ctx, |ui| {
                ui.add_space(10.);
                ui.strong("Outline");
                ui.separator();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    for (pos, title) in window::outline(&tab.doc.text) {
                        if ui.selectable_label(false, title).clicked() {
                            self.jump = Some(pos..pos);
                        }
                    }
                });
            });
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.weak(format!("{} words", tab.doc.text.split_whitespace().count()));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.selectable_value(&mut tab.source, true, "Source");
                    ui.selectable_value(&mut tab.source, false, "Write");
                });
            });
        });
        if !tab.source {
            let render = |ui: &mut egui::Ui| {
                ui.strong("Preview");
                ui.separator();
                egui::ScrollArea::vertical()
                    .id_salt(("preview", tab.id))
                    .show(ui, |ui| {
                        let base = tab.doc.path.as_deref().and_then(|p| p.parent());
                        window::preview(ui, &mut self.preview_cache, &tab.doc.text, base);
                    });
            };
            if ctx.available_rect().width() > 610. {
                egui::SidePanel::right("preview")
                    .default_width(ctx.available_rect().width() * 0.47)
                    .show(ctx, render);
            } else {
                egui::TopBottomPanel::bottom("preview")
                    .resizable(true)
                    .default_height(ctx.available_rect().height() * 0.4)
                    .show(ctx, render);
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
            let target = self.jump.take();
            if let Some(r) = &target {
                let mut state = TextEdit::load_state(ctx, id).unwrap_or_default();
                state.cursor.set_char_range(Some(CCursorRange::two(
                    CCursor::new(r.start),
                    CCursor::new(r.end),
                )));
                state.store(ctx, id);
                ctx.memory_mut(|m| m.request_focus(id));
            }
            egui::ScrollArea::vertical().id_salt(tab.id).show(ui, |ui| {
                ui.add_space(14.);
                let caret = target.as_ref().unwrap_or(&tab.selection).end;
                let mut layouter = |ui: &egui::Ui, s: &str, w: f32| {
                    ui.fonts(|f| {
                        f.layout_job(window::layout(
                            s,
                            w,
                            ui.visuals().text_color(),
                            tab.source,
                            caret,
                        ))
                    })
                };

                let has_focus = ui.memory(|m| m.has_focus(id));
                let mut choose = false;
                let mut escape = false;
                if has_focus
                    && !tab.source
                    && let Some(r) = slash::query(&tab.doc.text, &tab.selection)
                {
                    let query = tab.doc.text[r.start + 1..r.end].to_lowercase();
                    let count = slash::COMMANDS
                        .iter()
                        .filter(|c| c.name.to_ascii_lowercase().contains(&query))
                        .count();
                    if count > 0 && tab.dismissed.as_ref() != Some(&(r.start, query.to_owned())) {
                        ui.input_mut(|i| {
                            if i.consume_key(Modifiers::NONE, Key::ArrowDown) {
                                tab.slash_index = (tab.slash_index + 1) % count;
                            }
                            if i.consume_key(Modifiers::NONE, Key::ArrowUp) {
                                tab.slash_index =
                                    (tab.slash_index.min(count - 1) + count - 1) % count;
                            }
                            choose = i.consume_key(Modifiers::NONE, Key::Enter)
                                || i.consume_key(Modifiers::NONE, Key::Tab);
                            escape = i.consume_key(Modifiers::NONE, Key::Escape);
                        });
                    }
                }
                let output = TextEdit::multiline(&mut tab.doc.text)
                    .id(id)
                    .frame(false)
                    .desired_width(f32::INFINITY)
                    .desired_rows(24)
                    .layouter(&mut layouter)
                    .show(ui);
                if let Some(cursor) = output.cursor_range {
                    tab.selection = cursor.as_sorted_char_range();
                }
                if let Some(target) = target {
                    let c = output.galley.from_ccursor(CCursor::new(target.start));
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
                if let Some(r) = slash::query(&tab.doc.text, &tab.selection)
                    .filter(|_| !tab.source && (has_focus || output.response.has_focus()))
                {
                    let query = tab.doc.text[r.start + 1..r.end].to_lowercase();
                    if escape {
                        tab.dismissed = Some((r.start, query.clone()));
                    }
                    if tab.dismissed.as_ref() != Some(&(r.start, query.clone())) {
                        let matches: Vec<_> = slash::COMMANDS
                            .iter()
                            .filter(|c| c.name.to_lowercase().contains(&query))
                            .collect();
                        if !matches.is_empty() {
                            ui.memory_mut(|m| {
                                m.set_focus_lock_filter(
                                    id,
                                    egui::EventFilter {
                                        escape: true,
                                        tab: true,
                                        horizontal_arrows: true,
                                        vertical_arrows: true,
                                    },
                                )
                            });
                            tab.slash_index = tab.slash_index.min(matches.len() - 1);
                            let caret = output.galley.from_ccursor(CCursor::new(tab.selection.end));
                            let rect = output
                                .galley
                                .pos_from_cursor(&caret)
                                .translate(output.galley_pos.to_vec2());
                            let above = ctx.screen_rect().bottom() - rect.bottom() < 230.
                                && rect.top() > 230.;
                            let mut picked = choose.then_some(tab.slash_index);
                            egui::Area::new(id.with("slash"))
                                .order(egui::Order::Foreground)
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
                                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                                        egui::ScrollArea::vertical().max_height(210.).show(
                                            ui,
                                            |ui| {
                                                for (i, c) in matches.iter().enumerate() {
                                                    let row = ui.selectable_label(
                                                        i == tab.slash_index,
                                                        c.name,
                                                    );
                                                    if i == tab.slash_index {
                                                        row.scroll_to_me(None);
                                                    }
                                                    if row.clicked() {
                                                        picked = Some(i);
                                                    }
                                                }
                                            },
                                        );
                                    });
                                });
                            if let Some(i) = picked {
                                let pos = slash::insert(&mut tab.doc.text, r, matches[i]);
                                self.jump = Some(pos..pos);
                                tab.selection = pos..pos;
                                tab.doc.dirty = true;
                                tab.edited = Some(Instant::now());
                                ctx.request_repaint();
                            }
                        }
                    }
                }
                if output.response.changed() {
                    tab.doc.dirty = true;
                    tab.edited = Some(Instant::now());
                }
            });
        });
    }
}

impl eframe::App for Editor {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if ctx.input(|i| i.viewport().close_requested())
            && !self.quit
            && self.tabs.iter().any(|t| t.doc.dirty)
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.finish_close(ctx, Closing::App);
        }
        if self.closing.is_none() && self.error.is_none() {
            self.shortcuts(ctx);
            let drops: Vec<PathBuf> = ctx.input(|i| {
                i.raw
                    .dropped_files
                    .iter()
                    .filter_map(|f| f.path.clone())
                    .collect()
            });
            for path in drops {
                self.open(&path);
            }
        }
        self.toolbar(ctx);
        self.editor(ctx);
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
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(format!(
            "{}{} — Folio",
            tab.doc.name(),
            if tab.doc.dirty { " *" } else { "" }
        )));
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
                        f.layout_job(window::layout(s, w, ui.visuals().text_color(), source, 0))
                    })
                };
                let field = TextEdit::multiline(&mut doc.text)
                    .layouter(&mut layout)
                    .show(ui);
                assert_eq!(field.galley.text(), doc.text);
                if !source {
                    window::preview(ui,&mut Default::default(),"Heading\n=======\n\n| A | B |\n|---|---|\n| **one** | ~~two~~ |\n\n> quote\n\n- [x] done\n\nA note[^1]\n\n[^1]: footnote",None);
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
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(Editor::new()))
        }),
    )
}
