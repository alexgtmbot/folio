mod document;
mod window;

use document::Document;
use objc2::{
    ClassType, MainThreadOnly, define_class, msg_send, rc::Retained, runtime::ProtocolObject, sel,
};
use objc2_app_kit::*;
use objc2_foundation::*;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    struct AppDelegate;
    unsafe impl NSObjectProtocol for AppDelegate {}
    unsafe impl NSApplicationDelegate for AppDelegate {
        #[unsafe(method(applicationDidFinishLaunching:))]
        fn launched(&self, _: &NSNotification) {
            let mtm = self.mtm();
            let app = NSApplication::sharedApplication(mtm);
            let bar = NSMenu::new(mtm);
            for (title, items) in [
                ("Folio", vec![("Quit Folio", sel!(terminate:), "q", 0)]),
                (
                    "File",
                    vec![
                        ("New", sel!(newDocument:), "n", 0),
                        ("Open…", sel!(openDocument:), "o", 0),
                        ("Save", sel!(saveDocument:), "s", 0),
                        ("Close", sel!(performClose:), "w", 0),
                    ],
                ),
                (
                    "Edit",
                    vec![
                        ("Undo", sel!(undo:), "z", 0),
                        ("Redo", sel!(redo:), "Z", 0),
                        ("Cut", sel!(cut:), "x", 0),
                        ("Copy", sel!(copy:), "c", 0),
                        ("Paste", sel!(paste:), "v", 0),
                        ("Select All", sel!(selectAll:), "a", 0),
                        ("Find…", sel!(performFindPanelAction:), "f", 1),
                    ],
                ),
                (
                    "Format",
                    vec![
                        ("Bold", sel!(wrap:), "b", 0),
                        ("Italic", sel!(wrap:), "i", 1),
                        ("Inline Code", sel!(wrap:), "e", 2),
                        ("Toggle Task", sel!(task:), "T", 0),
                        ("Paragraph", sel!(block:), "0", 0),
                        ("Heading 1", sel!(block:), "1", 1),
                        ("Heading 2", sel!(block:), "2", 2),
                        ("Heading 3", sel!(block:), "3", 3),
                        ("Bullet List", sel!(block:), "", 4),
                        ("Quote", sel!(block:), "", 5),
                        ("Toggle Markdown Source", sel!(toggleSource:), "/", 0),
                    ],
                ),
            ] {
                let menu = NSMenu::initWithTitle(NSMenu::alloc(mtm), &NSString::from_str(title));
                let parent = NSMenuItem::new(mtm);
                parent.setSubmenu(Some(&menu));
                bar.addItem(&parent);
                for (name, action, key, tag) in items {
                    let item = unsafe {
                        menu.addItemWithTitle_action_keyEquivalent(
                            &NSString::from_str(name),
                            Some(action),
                            &NSString::from_str(key),
                        )
                    };
                    item.setTag(tag);
                }
            }
            app.setMainMenu(Some(&bar));
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
        }
    }
);

fn main() {
    objc2::rc::autoreleasepool(|_| {
        let mtm = MainThreadMarker::new().expect("AppKit needs the main thread");
        let app = NSApplication::sharedApplication(mtm);
        Document::class();
        if std::env::args().any(|a| a == "--self-test") {
            document::self_test(mtm);
            return;
        }
        let delegate: Retained<AppDelegate> = unsafe { msg_send![AppDelegate::class(), new] };
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
        app.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        app.run();
    })
}
