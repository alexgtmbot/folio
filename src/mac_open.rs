//! Receive Finder's open-documents Apple event on the UI thread.
use std::{cell::RefCell, ffi::c_void, path::PathBuf};

thread_local! {
    static PATHS: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    static CONTEXT: RefCell<Option<eframe::egui::Context>> = const { RefCell::new(None) };
}

// AEDataModel.h uses two-byte packing even on 64-bit macOS.
#[repr(C, packed(2))]
struct Desc(u32, *mut c_void);

impl Desc {
    fn empty() -> Self {
        Self(u32::from_be_bytes(*b"null"), std::ptr::null_mut())
    }
}
impl Drop for Desc {
    fn drop(&mut self) {
        unsafe {
            AEDisposeDesc(self);
        }
    }
}

#[link(name = "CoreServices", kind = "framework")]
unsafe extern "C" {
    fn AEInstallEventHandler(
        class: u32,
        id: u32,
        callback: extern "C" fn(*const Desc, *mut Desc, isize) -> i16,
        data: isize,
        system: u8,
    ) -> i16;
    fn AEGetParamDesc(event: *const Desc, key: u32, kind: u32, result: *mut Desc) -> i16;
    fn AECountItems(list: *const Desc, count: *mut isize) -> i16;
    fn AEGetNthDesc(
        list: *const Desc,
        index: isize,
        kind: u32,
        key: *mut u32,
        result: *mut Desc,
    ) -> i16;
    fn AEGetDescDataSize(desc: *const Desc) -> isize;
    fn AEGetDescData(desc: *const Desc, bytes: *mut u8, size: isize) -> i16;
    fn AEDisposeDesc(desc: *mut Desc) -> i16;
}

fn check(status: i16) -> Result<(), i16> {
    if status == 0 { Ok(()) } else { Err(status) }
}

fn paths(event: *const Desc) -> Result<Vec<PathBuf>, i16> {
    let mut list = Desc::empty();
    let mut count = 0;
    let mut paths = Vec::new();
    unsafe {
        check(AEGetParamDesc(
            event,
            u32::from_be_bytes(*b"----"),
            u32::from_be_bytes(*b"list"),
            &mut list,
        ))?;
        check(AECountItems(&list, &mut count))?;
        for index in 1..=count {
            let mut url = Desc::empty();
            check(AEGetNthDesc(
                &list,
                index,
                u32::from_be_bytes(*b"furl"),
                std::ptr::null_mut(),
                &mut url,
            ))?;
            let size = AEGetDescDataSize(&url);
            let mut bytes = vec![0; usize::try_from(size).map_err(|_| -1700i16)?];
            check(AEGetDescData(&url, bytes.as_mut_ptr(), size))?;
            let path = std::str::from_utf8(&bytes)
                .ok()
                .and_then(|s| url::Url::parse(s).ok())
                .and_then(|u| u.to_file_path().ok())
                .ok_or(-1700i16)?;
            paths.push(path);
        }
    }
    Ok(paths)
}

extern "C" fn receive(event: *const Desc, _: *mut Desc, _: isize) -> i16 {
    match paths(event) {
        Ok(paths) => {
            PATHS.with_borrow_mut(|pending| pending.extend(paths));
            CONTEXT.with_borrow(|ctx| {
                if let Some(ctx) = ctx {
                    ctx.request_repaint();
                }
            });
            0
        }
        Err(code) => code,
    }
}

pub fn attach(ctx: &eframe::egui::Context) {
    CONTEXT.with_borrow_mut(|slot| *slot = Some(ctx.clone()));
}

pub fn on_launch() {
    use core_foundation::{base::TCFType, string::CFString};
    use core_foundation_sys::{dictionary::CFDictionaryRef, notification_center::*};
    extern "C" fn will_finish(
        _: CFNotificationCenterRef,
        _: *mut c_void,
        _: CFNotificationName,
        _: *const c_void,
        _: CFDictionaryRef,
    ) {
        if let Err(error) = install() {
            eprintln!("{error}");
        }
    }
    let name = CFString::new("NSApplicationWillFinishLaunchingNotification");
    unsafe {
        CFNotificationCenterAddObserver(
            CFNotificationCenterGetLocalCenter(),
            std::ptr::null(),
            will_finish,
            name.as_concrete_TypeRef(),
            std::ptr::null(),
            CFNotificationSuspensionBehaviorDeliverImmediately,
        );
    }
}

pub fn install() -> std::io::Result<()> {
    check(unsafe {
        AEInstallEventHandler(
            u32::from_be_bytes(*b"aevt"),
            u32::from_be_bytes(*b"odoc"),
            receive,
            0,
            0,
        )
    })
    .map_err(|code| {
        std::io::Error::other(format!(
            "Could not register Finder document handler ({code})"
        ))
    })
}

pub fn pending() -> Vec<PathBuf> {
    PATHS.take()
}
