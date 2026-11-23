//! Read PNG-only pasteboards. arboard's macOS image reader requests TIFF.
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    data::{CFData, CFDataRef},
    string::{CFString, CFStringRef},
};
use std::{ffi::c_void, ptr};

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn PasteboardCreate(name: CFStringRef, board: *mut CFTypeRef) -> i32;
    fn PasteboardGetItemIdentifier(board: CFTypeRef, index: isize, item: *mut *mut c_void) -> i32;
    fn PasteboardCopyItemFlavorData(
        board: CFTypeRef,
        item: *mut c_void,
        kind: CFStringRef,
        data: *mut CFDataRef,
    ) -> i32;
}

pub fn read() -> Option<Vec<u8>> {
    let name = CFString::new("com.apple.pasteboard.clipboard");
    let kind = CFString::new("public.png");
    let mut board = ptr::null();
    let mut item = ptr::null_mut();
    let mut data = ptr::null();
    // The Create and Copy functions transfer ownership on success. Their CF
    // wrappers keep the board and bytes alive until the owned copy is complete.
    unsafe {
        if PasteboardCreate(name.as_concrete_TypeRef(), &mut board) != 0 || board.is_null() {
            return None;
        }
        let board = CFType::wrap_under_create_rule(board);
        if PasteboardGetItemIdentifier(board.as_CFTypeRef(), 1, &mut item) != 0
            || PasteboardCopyItemFlavorData(
                board.as_CFTypeRef(),
                item,
                kind.as_concrete_TypeRef(),
                &mut data,
            ) != 0
            || data.is_null()
        {
            return None;
        }
        Some(CFData::wrap_under_create_rule(data).bytes().to_vec())
    }
}
