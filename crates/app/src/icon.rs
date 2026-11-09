//! Application icon.
//!
//! A bundled macOS app gets its Dock icon from `Contents/Resources`, but the
//! bare binary built by `cargo run` inherits the generic executable icon. The
//! same PNG is applied at startup, so both look the same.
//!
//! The window needs it too: without an icon in the viewport eframe puts its
//! own (a black hexagon) on the Windows taskbar and over the macOS Dock icon.

/// macOS draws the Dock icon as given, so it has the margin and shadow of the
/// other icons; elsewhere the icon fills the taskbar slot.
#[cfg(target_os = "macos")]
const PNG_512: &[u8] = include_bytes!("../../../deployment/fotogramma.iconset/icon_256x256@2x.png");
#[cfg(not(target_os = "macos"))]
const PNG_512: &[u8] = include_bytes!("../../../deployment/Fotogramma-window.png");

/// The icon of the window, for the taskbar and the title bar.
pub fn window_icon() -> Option<eframe::egui::IconData> {
    let image = image::load_from_memory(PNG_512).ok()?.to_rgba8();
    Some(eframe::egui::IconData {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}

#[cfg(target_os = "macos")]
pub fn install() {
    use objc2::AnyThread;
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(PNG_512);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
    }
}

#[cfg(not(target_os = "macos"))]
pub fn install() {}

#[cfg(test)]
mod tests {
    #[test]
    fn the_window_icon_is_the_fotogramma_icon() {
        let icon = super::window_icon().expect("the bundled PNG decodes");
        assert_eq!(icon.width, icon.height);
        assert_eq!(icon.rgba.len(), (icon.width * icon.height * 4) as usize);
        let middle = ((icon.height / 2 * icon.width + icon.width / 2) * 4) as usize;
        assert!(
            icon.rgba[middle + 3] > 0,
            "opaque inside the rounded square"
        );
        assert_eq!(icon.rgba[3], 0, "the corner is transparent");
    }
}
