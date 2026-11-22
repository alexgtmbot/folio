use super::document::Document;
use objc2::{DefinedClass, MainThreadOnly, define_class, msg_send, rc::Retained, sel};
use objc2_app_kit::*;
use objc2_foundation::*;

define_class!(
    #[unsafe(super(NSView))]
    struct OutlineView;
    impl OutlineView {
        #[unsafe(method(isFlipped))]
        fn flipped(&self) -> bool { true }
    }
);

fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
}

pub fn build(doc: &Document) {
    let mtm = doc.mtm();
    let state = doc.ivars();
    let window = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            NSWindow::alloc(mtm),
            rect(0., 0., 940., 660.),
            NSWindowStyleMask::Titled
                | NSWindowStyleMask::Closable
                | NSWindowStyleMask::Miniaturizable
                | NSWindowStyleMask::Resizable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    let root = window.contentView().unwrap();
    let width = NSAutoresizingMaskOptions::ViewWidthSizable;
    let height = NSAutoresizingMaskOptions::ViewHeightSizable;
    let top = NSAutoresizingMaskOptions::ViewMinYMargin;
    let editor = doc.editor();
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(176., 36., 764., 624.));
    scroll.setAutoresizingMask(width | height);
    scroll.setHasVerticalScroller(true);
    scroll.setAutohidesScrollers(true);
    editor.setRichText(false);
    editor.setAllowsUndo(true);
    editor.setUsesFindBar(true);
    editor.setAutomaticQuoteSubstitutionEnabled(false);
    editor.setAutomaticDashSubstitutionEnabled(false);
    editor.setAutomaticTextReplacementEnabled(false);
    editor.setTextContainerInset(NSSize::new(42., 34.));
    editor.setVerticallyResizable(true);
    editor.setFrame(rect(
        0.,
        0.,
        scroll.contentSize().width,
        scroll.contentSize().height,
    ));
    editor.setAutoresizingMask(width);
    editor.setFont(Some(&NSFont::systemFontOfSize(17.)));
    unsafe {
        if let Some(container) = editor.textContainer() {
            container.setWidthTracksTextView(true);
        }
        let _: () = msg_send![editor,setDelegate:doc];
    }
    scroll.setDocumentView(Some(editor));
    root.addSubview(&scroll);
    let caption = NSTextField::labelWithString(&NSString::from_str("Outline"), mtm);
    caption.setFont(Some(&NSFont::boldSystemFontOfSize(14.)));
    caption.setFrame(rect(16., 615., 150., 25.));
    caption.setAutoresizingMask(top);
    root.addSubview(&caption);
    let rail = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), rect(8., 5., 160., 604.));
    rail.setDrawsBackground(false);
    rail.setHasVerticalScroller(true);
    rail.setAutohidesScrollers(true);
    rail.setAutoresizingMask(height);
    let outline: Retained<OutlineView> =
        unsafe { msg_send![OutlineView::alloc(mtm), initWithFrame:rect(0.,0.,150.,0.)] };
    rail.setDocumentView(Some(&outline));
    root.addSubview(&rail);
    state.outline.set(outline.into_super()).unwrap();
    let status = NSTextField::labelWithString(ns_string!(""), mtm);
    status.setFrame(rect(196., 7., 200., 21.));
    status.setTextColor(Some(&NSColor::secondaryLabelColor()));
    root.addSubview(&status);
    state.status.set(status).unwrap();
    let mode = NSSegmentedControl::new(mtm);
    mode.setSegmentCount(2);
    unsafe {
        mode.setLabel_forSegment(ns_string!("Write"), 0);
        mode.setLabel_forSegment(ns_string!("Source"), 1);
        mode.setTarget(Some(doc));
        mode.setAction(Some(sel!(chooseMode:)));
    }
    mode.setSelectedSegment(0);
    mode.setFrame(rect(793., 4., 135., 28.));
    mode.setAutoresizingMask(NSAutoresizingMaskOptions::ViewMinXMargin);
    root.addSubview(&mode);
    state.mode.set(mode).unwrap();
    window.setContentMinSize(NSSize::new(560., 300.));
    window.center();
    let controller =
        NSWindowController::initWithWindow(NSWindowController::alloc(mtm), Some(&window));
    doc.addWindowController(&controller);
    window.makeFirstResponder(Some(editor));
    doc.style();
}

pub fn outline(doc: &Document, entries: Vec<(usize, String)>) {
    let state = doc.ivars();
    let Some(view) = state.outline.get() else {
        return;
    };
    if *state.headings.borrow() == entries {
        return;
    }
    for sub in view.subviews().iter() {
        sub.removeFromSuperview();
    }
    view.setFrameSize(NSSize::new(150., entries.len() as f64 * 28.));
    for (i, (offset, title)) in entries.iter().enumerate() {
        let button = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(title),
                Some(doc),
                Some(sel!(jump:)),
                doc.mtm(),
            )
        };
        button.setBordered(false);
        button.setTag(*offset as isize);
        button.setAlignment(NSTextAlignment::Left);
        button.setFrame(rect(2., i as f64 * 28., 146., 26.));
        button.setToolTip(Some(&NSString::from_str(title)));
        view.addSubview(&button);
    }
    *state.headings.borrow_mut() = entries;
}
