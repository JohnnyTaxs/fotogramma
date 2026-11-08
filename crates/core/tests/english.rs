//! English output. This lives in its own test binary because the language is
//! a process-wide setting and the unit tests assert the Italian default.

use fotogramma_core::formatting::{codec_label, format_bytes, format_int};
use fotogramma_core::i18n::{set_lang, Lang};
use fotogramma_core::scanner::{Category, VideoInfo};
use std::path::PathBuf;

fn video(category: Category) -> VideoInfo {
    VideoInfo {
        hdr: false,
        path: PathBuf::from("/v/a.mkv"),
        relative_path: "a.mkv".into(),
        name: "a.mkv".into(),
        folder: String::new(),
        extension: "mkv".into(),
        category,
        codec: Some("h264".into()),
        size: Some(1),
        duration: None,
        width: None,
        height: None,
        error: None,
    }
}

#[test]
fn english_uses_english_separators_labels_and_messages() {
    set_lang(Lang::En);
    assert_eq!(format_int(1_234_567), "1,234,567");
    assert_eq!(format_bytes(Some(1536)), "1.5 KB");
    assert_eq!(codec_label(None), "Unknown");
    assert_eq!(video(Category::Convert).status_label(), "Convert to H.265");
    assert_eq!(video(Category::Compatible).status_label(), "Ready");
    assert_eq!(video(Category::Compatible).folder_label(), "Main folder");

    let profiles = fotogramma_core::conversion::builtin_profiles();
    assert!(profiles
        .iter()
        .any(|p| p.label == "Intermediate · good quality"));
    let custom = fotogramma_core::conversion::custom_profile(
        22,
        "slow",
        "yuv420p",
        None,
        None,
        fotogramma_core::conversion::AudioMode::Copy,
        128,
    );
    assert_eq!(custom.label, "Custom");

    let (replaced, message) = fotogramma_core::verification::replace_verified_output(
        std::path::Path::new("/"),
        std::path::Path::new("/tmp/x"),
        std::path::Path::new("/missing"),
    );
    assert!(!replaced);
    assert_eq!(message, "Invalid original path");
}
