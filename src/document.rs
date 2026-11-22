use folio::{Fence, FileFormat, block_prefix, byte_at, inline, task_prefix};
use objc2::{
    ClassType, DefinedClass, MainThreadOnly, define_class, msg_send,
    rc::{Allocated, Retained},
    runtime::AnyObject,
};
use objc2_app_kit::*;
use objc2_foundation::*;
use std::cell::{Cell, OnceCell, RefCell};

#[derive(Default)]
pub(crate) struct State {
    pub editor: OnceCell<Retained<NSTextView>>,
    pub outline: OnceCell<Retained<NSView>>,
    pub status: OnceCell<Retained<NSTextField>>,
    pub mode: OnceCell<Retained<NSSegmentedControl>>,
    pub headings: RefCell<Vec<(usize, String)>>,
    format: Cell<FileFormat>,
    pub source: Cell<bool>,
    styling: Cell<bool>,
}

define_class!(
    #[unsafe(super(NSDocument))]
    #[name = "FolioDocument"]
    #[ivars = State]
    pub(crate) struct Document;
    impl Document {
        #[unsafe(method_id(init))]
        fn init(this: Allocated<Self>) -> Retained<Self> {
            let this = this.set_ivars(State::default());
            unsafe { msg_send![super(this), init] }
        }
        #[unsafe(method(autosavesInPlace))]
        fn autosave() -> bool { true }
        #[unsafe(method(readFromData:ofType:error:))]
        unsafe fn read(&self, data: &NSData, _: &NSString, error: *mut *mut NSError) -> bool {
            match FileFormat::decode(&data.to_vec()) {
                Ok((text, format)) => {
                    self.ivars().format.set(format);
                    self.editor().setString(&NSString::from_str(&text));
                    self.style();
                    true
                }
                Err(_) => {
                    if !error.is_null() {
                        let e = unsafe { NSError::errorWithDomain_code_userInfo(ns_string!("NSCocoaErrorDomain"),261,None) };
                        unsafe { *error = Retained::autorelease_return(e) };
                    }
                    false
                }
            }
        }
        #[unsafe(method_id(dataOfType:error:))]
        fn data(&self, _: &NSString, _: *mut *mut NSError) -> Retained<NSData> {
            NSData::with_bytes(&self.ivars().format.get().encode(&self.editor().string().to_string()))
        }
        #[unsafe(method(makeWindowControllers))]
        fn windows(&self) { crate::window::build(self); }
        #[unsafe(method(textDidChange:))]
        fn changed(&self, _: &NSNotification) { self.style(); }
        #[unsafe(method(textViewDidChangeSelection:))]
        fn selection(&self, _: &NSNotification) { self.style(); }
        #[unsafe(method(toggleSource:))]
        fn toggle(&self, _: Option<&AnyObject>) {
            let source = !self.ivars().source.get();
            self.ivars().source.set(source);
            if let Some(mode) = self.ivars().mode.get() { mode.setSelectedSegment(source as isize); }
            self.style();
        }
        #[unsafe(method(chooseMode:))]
        fn choose(&self, sender: &NSSegmentedControl) {
            self.ivars().source.set(sender.selectedSegment() == 1);
            self.style();
        }
        #[unsafe(method(jump:))]
        fn jump(&self, sender: &NSButton) {
            let range = NSRange::new(sender.tag() as usize,0);
            self.editor().setSelectedRange(range);
            self.editor().scrollRangeToVisible(range);
            if let Some(window) = self.editor().window() { window.makeFirstResponder(Some(self.editor())); }
        }
        #[unsafe(method(wrap:))]
        fn wrap(&self, sender: &NSMenuItem) {
            let Some(mark) = ["**","*","`"].get(sender.tag() as usize).copied() else { return };
            let range = self.editor().selectedRange();
            let selected = self.editor().string().substringWithRange(range);
            self.replace(range, &format!("{mark}{selected}{mark}"));
            self.editor().setSelectedRange(NSRange::new(range.location+mark.len(),range.length));
        }
        #[unsafe(method(task:))]
        fn task(&self, _: Option<&AnyObject>) { self.change_prefix(None); }
        #[unsafe(method(block:))]
        fn block(&self, sender: &NSMenuItem) {
            if let Some(prefix) = ["","# ","## ","### ","- ","> "].get(sender.tag() as usize) {
                self.change_prefix(Some(prefix));
            }
        }
    }
);

impl Document {
    pub fn editor(&self) -> &NSTextView {
        self.ivars()
            .editor
            .get_or_init(|| NSTextView::new(self.mtm()))
    }
    fn replace(&self, range: NSRange, text: &str) {
        unsafe {
            let _: () = msg_send![self.editor(), insertText: &*NSString::from_str(text), replacementRange: range];
        }
    }
    fn change_prefix(&self, new: Option<&str>) {
        let text = self.editor().string().to_string();
        let pos = byte_at(&text, self.editor().selectedRange().location);
        let start = text[..pos].rfind(['\r', '\n']).map_or(0, |i| i + 1);
        let line = &text[start..];
        let (count, prefix) = new.map_or_else(|| task_prefix(line), |p| (block_prefix(line), p));
        self.replace(
            NSRange::new(text[..start].encode_utf16().count(), count),
            prefix,
        );
    }
    pub fn style(&self) {
        let state = self.ivars();
        if state.styling.replace(true) {
            return;
        }
        let editor = self.editor();
        let text = editor.string().to_string();
        let selection = editor.selectedRange().location;
        let source = state.source.get();
        let mut headings = Vec::new();
        let mut offset = 0;
        let mut fence = Fence::default();
        if let Some(storage) = unsafe { editor.textStorage() } {
            let body = NSFont::systemFontOfSize(17.);
            let mono = NSFont::monospacedSystemFontOfSize_weight(15., 0.);
            let tiny = NSFont::systemFontOfSize(0.1);
            let color = NSColor::textColor();
            let clear = NSColor::clearColor();
            let paragraph = NSMutableParagraphStyle::new();
            paragraph.setParagraphSpacing(if source { 0. } else { 4. });
            paragraph.setLineSpacing(if source { 1. } else { 2. });
            let full = NSRange::new(0, text.encode_utf16().count());
            unsafe {
                storage.beginEditing();
                storage.setAttributes_range(None, full);
                storage.addAttribute_value_range(ns_string!("NSParagraphStyle"), &paragraph, full);
                storage.addAttribute_value_range(ns_string!("NSColor"), &color, full);
            }
            for line in text.split_inclusive('\n') {
                let len = line.encode_utf16().count();
                let range = NSRange::new(offset, len);
                let code = fence.contains(line);
                let n = block_prefix(line);
                let heading = !code && n > 0 && line.starts_with('#');
                if heading {
                    headings.push((offset, line[n..].trim().to_string()));
                }
                let font = if source || code {
                    mono.clone()
                } else if heading {
                    NSFont::systemFontOfSize_weight(34. - 3. * (n - 2) as f64, 0.3)
                } else {
                    body.clone()
                };
                unsafe {
                    storage.addAttribute_value_range(ns_string!("NSFont"), &font, range);
                    if !source && code {
                        storage.addAttribute_value_range(
                            ns_string!("NSBackgroundColor"),
                            &NSColor::quaternaryLabelColor(),
                            range,
                        );
                    }
                }
                if !source && !code {
                    let active = (offset..offset + len).contains(&selection)
                        || selection == offset + len && !line.ends_with('\n');
                    if n > 0 {
                        unsafe {
                            let prefix = NSRange::new(offset, n);
                            storage.addAttribute_value_range(
                                ns_string!("NSColor"),
                                &NSColor::secondaryLabelColor(),
                                prefix,
                            );
                            if heading && !active {
                                storage.addAttribute_value_range(
                                    ns_string!("NSColor"),
                                    &clear,
                                    prefix,
                                );
                                storage.addAttribute_value_range(
                                    ns_string!("NSFont"),
                                    &tiny,
                                    prefix,
                                );
                            }
                        }
                    }
                    inline(line, |r, marker| {
                        let start = offset + line[..r.start].encode_utf16().count();
                        let length = line[r].encode_utf16().count();
                        let face = if marker == "`" {
                            mono.clone()
                        } else if marker.len() == 2 {
                            NSFont::boldSystemFontOfSize(font.pointSize())
                        } else {
                            NSFontManager::sharedFontManager(self.mtm())
                                .convertFont_toHaveTrait(&font, NSFontTraitMask::ItalicFontMask)
                        };
                        unsafe {
                            storage.addAttribute_value_range(
                                ns_string!("NSFont"),
                                &face,
                                NSRange::new(start, length),
                            );
                            if !(start..start + length).contains(&selection) {
                                for loc in [start, start + length - marker.len()] {
                                    let range = NSRange::new(loc, marker.len());
                                    storage.addAttribute_value_range(
                                        ns_string!("NSFont"),
                                        &tiny,
                                        range,
                                    );
                                    storage.addAttribute_value_range(
                                        ns_string!("NSColor"),
                                        &clear,
                                        range,
                                    );
                                }
                            }
                        }
                    });
                }
                offset += len;
            }
            storage.endEditing();
        }
        if let Some(status) = state.status.get() {
            status.setStringValue(&NSString::from_str(&format!(
                "{} words",
                text.split_whitespace().count()
            )));
        }
        crate::window::outline(self, headings);
        state.styling.set(false);
    }
}

pub fn self_test(mtm: MainThreadMarker) {
    let doc: Retained<Document> = unsafe { msg_send![Document::class(), new] };
    let md = ns_string!("Markdown");
    let fixture = "\u{feff}# 🦀 Café\r\n- item\r\n";
    doc.readFromData_ofType_error(&NSData::with_bytes(fixture.as_bytes()), md)
        .unwrap();
    doc.makeWindowControllers();
    assert_eq!(
        doc.dataOfType_error(md).unwrap().to_vec(),
        fixture.as_bytes()
    );
    assert_eq!(doc.ivars().headings.borrow()[0].1, "🦀 Café");
    doc.editor().setSelectedRange(NSRange::new(5, 4));
    let item = NSMenuItem::new(mtm);
    unsafe {
        let _: () = msg_send![&*doc,wrap:&*item];
    }
    assert!(doc.editor().string().to_string().contains("**Café**"));
    let undo = doc.editor().undoManager().expect("document undo manager");
    assert!(undo.canUndo());
    undo.undo();
    assert!(!doc.editor().string().to_string().contains("**Café**"));
    undo.redo();
    assert!(doc.editor().string().to_string().contains("**Café**"));
    item.setTag(2);
    unsafe {
        let _: () = msg_send![&*doc,block:&*item];
    }
    assert!(doc.editor().string().to_string().starts_with("## "));
    unsafe {
        let _: () = msg_send![&*doc,task:&*item];
    }
    unsafe {
        let _: () = msg_send![&*doc,task:&*item];
    }
    assert!(doc.editor().string().to_string().starts_with("- [x] "));
    unsafe {
        let _: () = msg_send![&*doc,toggleSource:&*item];
    }
    assert!(doc.ivars().source.get());
    let directory = std::env::temp_dir().join(NSUUID::UUID().UUIDString().to_string());
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("Round trip.md");
    let url = NSURL::fileURLWithPath(&NSString::from_str(path.to_str().unwrap()));
    doc.writeSafelyToURL_ofType_forSaveOperation_error(
        &url,
        md,
        NSSaveOperationType::SaveOperation,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(&path).unwrap(),
        doc.dataOfType_error(md).unwrap().to_vec()
    );
    let reopened: Retained<Document> = unsafe { msg_send![Document::class(), new] };
    reopened.readFromURL_ofType_error(&url, md).unwrap();
    assert_eq!(reopened.editor().string(), doc.editor().string());
    assert!(
        reopened
            .readFromData_ofType_error(&NSData::with_bytes(&[0xff]), md)
            .is_err()
    );
    std::fs::remove_file(path).unwrap();
    std::fs::remove_dir(directory).unwrap();
    println!("Native document, UTF-8 round trip, outline, source and formatting checks passed");
}
