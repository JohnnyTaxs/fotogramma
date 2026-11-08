//! Application icon.
//!
//! A bundled macOS app gets its Dock icon from `Contents/Resources`, but the
//! bare binary built by `cargo run` inherits the generic executable icon. The
//! same PNG is applied at startup, so both look the same.

#[cfg(target_os = "macos")]
pub fn install() {
    use objc2::AnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    const PNG: &[u8] = include_bytes!("../../../deployment/fotogramma.iconset/icon_256x256@2x.png");

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(PNG);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}
