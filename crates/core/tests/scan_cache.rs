//! Scanning a real folder twice: the second pass comes from the cache.
//! Skipped when FFmpeg is not installed.

use fotogramma_core::cache::ScanCache;
use fotogramma_core::media_tools::{ffmpeg_path, ffprobe_path, prepare_command};
use fotogramma_core::scanner::{discover_videos, probe_videos_cached};
use std::path::{Path, PathBuf};
use std::process::Stdio;

fn make_clip(ffmpeg: &Path, directory: &Path, name: &str, frames: usize) -> PathBuf {
    let raw = directory.join(format!("{name}.yuv"));
    std::fs::write(&raw, vec![0u8; 64 * 64 * 3 / 2 * frames]).unwrap();
    let path = directory.join(name);
    let made = prepare_command(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(["-f", "rawvideo", "-pixel_format", "yuv420p"])
        .args(["-video_size", "64x64", "-framerate", "5", "-i"])
        .arg(&raw)
        .args(["-c:v", "mpeg4"])
        .arg(&path)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(made.success());
    std::fs::remove_file(raw).unwrap();
    path
}

#[test]
fn the_second_scan_reads_the_cache_and_a_changed_file_is_probed_again() {
    let (Ok(ffmpeg), Ok(ffprobe)) = (ffmpeg_path(), ffprobe_path()) else {
        eprintln!("skipped: ffmpeg and ffprobe are not available");
        return;
    };
    let root = std::env::temp_dir().join(format!("foto-scan-cache-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Show")).unwrap();
    let a = make_clip(&ffmpeg, &root, "a.mp4", 10);
    make_clip(&ffmpeg, &root.join("Show"), "b.mp4", 10);
    make_clip(&ffmpeg, &root, "c.mp4", 10);

    let scan = |cache: &ScanCache| {
        let candidates = discover_videos(&root, None, |_, _| {}, |_, _| {}).unwrap();
        let outcome = probe_videos_cached(
            &candidates,
            &root,
            &ffprobe,
            None,
            Some(cache),
            |_, _, _, _| {},
        );
        cache.finish(&root, !outcome.cancelled);
        outcome
    };

    let cache = ScanCache::default();
    let first = scan(&cache);
    assert_eq!(first.videos.len(), 3);
    assert_eq!(first.from_cache, 0);

    let second = scan(&cache);
    assert_eq!(
        second.videos, first.videos,
        "the cached results are the same"
    );
    assert_eq!(second.from_cache, 3);

    // A file that changes is probed again; the others stay cached.
    make_clip(&ffmpeg, &root, "a.mp4", 25);
    assert!(a.exists());
    let third = scan(&cache);
    assert_eq!(third.from_cache, 2);
    let changed = third.videos.iter().find(|v| v.name == "a.mp4").unwrap();
    assert!(
        changed.duration.unwrap() > 3.0,
        "the new duration, not the cached one"
    );

    // A deleted file disappears from the cache as well as from the results.
    std::fs::remove_file(root.join("Show").join("b.mp4")).unwrap();
    let fourth = scan(&cache);
    assert_eq!(fourth.videos.len(), 2);
    assert_eq!(cache.len(), 2);
    let _ = std::fs::remove_dir_all(&root);
}
