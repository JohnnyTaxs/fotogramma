//! End-to-end checks of the conversion safety rules with real FFmpeg binaries.
//!
//! These tests are skipped when FFmpeg is not installed. They never touch a
//! user file: every fixture lives in a private temporary directory.

use fotogramma_core::conversion::{
    allocate_output_path, allocate_partial_path, build_ffmpeg_args, profile,
};
use fotogramma_core::media_tools::{ffmpeg_path, ffprobe_path, prepare_command};
use fotogramma_core::scanner::{probe_video, Category};
use fotogramma_core::verification::{replace_verified_output, validate_conversion_output};
use std::path::{Path, PathBuf};
use std::process::Stdio;

fn tools() -> Option<(PathBuf, PathBuf)> {
    Some((ffmpeg_path().ok()?, ffprobe_path().ok()?))
}

fn temp_dir(name: &str) -> PathBuf {
    let directory =
        std::env::temp_dir().join(format!("fotogramma-pipeline-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&directory);
    std::fs::create_dir_all(&directory).expect("temporary directory");
    directory
}

fn run(command: &mut std::process::Command, what: &str) {
    let output = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap_or_else(|error| panic!("{what}: {error}"));
    assert!(
        output.status.success(),
        "{what}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// Pick an encoder the tested build actually provides. Development machines
/// have libx264; the packaged minimal FFmpeg ships the tiny mpeg4 encoder
/// instead, so the same safety pipeline can be exercised in a clean install.
fn video_encoder(ffmpeg: &Path) -> (&'static str, &'static str) {
    let output = prepare_command(ffmpeg)
        .args(["-hide_banner", "-encoders"])
        .output()
        .expect("ffmpeg -encoders");
    let text = String::from_utf8_lossy(&output.stdout);
    if text.contains(" libx264 ") {
        ("libx264", "h264")
    } else {
        ("mpeg4", "mpeg4")
    }
}

/// Create a two-second H.264 (or fallback) MP4 used by the conversion tests.
///
/// The source is written as raw YUV so the packaged minimal FFmpeg does not
/// need the `lavfi` device or any test-source filter to run this check.
fn make_source(ffmpeg: &Path, directory: &Path) -> (PathBuf, &'static str) {
    let (encoder, codec) = video_encoder(ffmpeg);
    let (width, height, frames) = (320usize, 240usize, 50usize);
    let mut raw = Vec::with_capacity(width * height * 3 / 2 * frames);
    for _ in 0..frames {
        raw.extend(std::iter::repeat_n(16u8, width * height)); // limited-range black
        raw.extend(std::iter::repeat_n(128u8, width * height / 2)); // neutral chroma
    }
    let raw_path = directory.join("source.yuv");
    std::fs::write(&raw_path, raw).expect("raw fixture");

    let source = directory.join("original.mp4");
    let mut command = prepare_command(ffmpeg);
    command
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pixel_format",
            "yuv420p",
            "-video_size",
            "320x240",
            "-framerate",
            "25",
            "-i",
        ])
        .arg(&raw_path)
        .args(["-c:v", encoder, "-pix_fmt", "yuv420p"])
        .arg(&source);
    run(&mut command, "generate fixture");
    (source, codec)
}

fn convert(ffmpeg: &Path, source: &Path, destination: &Path, profile_key: &str) {
    let selected = profile(profile_key).expect("profile");
    let args = build_ffmpeg_args(&selected, source, destination);
    let mut command = prepare_command(ffmpeg);
    command.args(args);
    run(&mut command, "ffmpeg conversion");
}

#[test]
fn converts_a_short_h264_to_hevc_without_touching_the_original() {
    let Some((ffmpeg, ffprobe)) = tools() else {
        eprintln!("FFmpeg not found: skipping media pipeline test");
        return;
    };
    let directory = temp_dir("copy");
    let (source, source_codec) = make_source(&ffmpeg, &directory);
    let original_bytes = std::fs::read(&source).expect("source bytes");

    let info = probe_video(&source, &directory, &ffprobe, None).expect("probe source");
    assert_eq!(info.category, Category::Convert);
    assert_eq!(info.codec.as_deref(), Some(source_codec));

    let output = allocate_output_path(&source, &directory, "mp4").expect("output path");
    convert(&ffmpeg, &source, &output, "fast");

    let (valid, message) = validate_conversion_output(&info, &output, &ffprobe, &ffmpeg);
    assert!(valid, "validation failed: {message}");

    let converted = probe_video(&output, &directory, &ffprobe, None).expect("probe output");
    assert_eq!(converted.category, Category::Compatible);
    assert_eq!(converted.codec.as_deref(), Some("hevc"));

    assert_eq!(
        std::fs::read(&source).expect("original still readable"),
        original_bytes,
        "the original must stay byte-identical in copy mode"
    );
    let _ = std::fs::remove_dir_all(&directory);
}

#[test]
fn replaces_the_original_only_after_full_verification() {
    let Some((ffmpeg, ffprobe)) = tools() else {
        eprintln!("FFmpeg not found: skipping media pipeline test");
        return;
    };
    let directory = temp_dir("replace");
    let (source, source_codec) = make_source(&ffmpeg, &directory);
    let info = probe_video(&source, &directory, &ffprobe, None).expect("probe source");
    assert_eq!(info.codec.as_deref(), Some(source_codec));
    assert_eq!(info.category, Category::Convert);

    let partial = allocate_partial_path(&source).expect("partial path");
    convert(&ffmpeg, &source, &partial, "fast");

    let (valid, message) = validate_conversion_output(&info, &partial, &ffprobe, &ffmpeg);
    assert!(valid, "validation failed: {message}");

    let (replaced, message) = replace_verified_output(&source, &partial, &ffprobe);
    assert!(replaced, "replacement failed: {message}");
    assert!(source.exists(), "the original path must hold the new file");
    assert!(!partial.exists(), "the partial file must be gone");

    let backup = directory.join("original.mp4.fotogramma-backup");
    assert!(!backup.exists(), "the backup must be removed after success");

    let final_info = probe_video(&source, &directory, &ffprobe, None).expect("probe final");
    assert_eq!(final_info.category, Category::Compatible);
    assert_eq!(final_info.codec.as_deref(), Some("hevc"));

    let _ = std::fs::remove_dir_all(&directory);
}
