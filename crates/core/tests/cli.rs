//! The real `fotogramma-cli` binary on a real folder. Skipped when FFmpeg is
//! not installed.

use fotogramma_core::media_tools::{ffmpeg_path, ffprobe_path, prepare_command};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn make_clip(ffmpeg: &Path, directory: &Path, name: &str, codec: &str) {
    let raw = directory.join(format!("{name}.yuv"));
    std::fs::write(&raw, vec![0u8; 64 * 64 * 3 / 2 * 10]).unwrap();
    let made = prepare_command(ffmpeg)
        .args(["-y", "-hide_banner", "-loglevel", "error"])
        .args(["-f", "rawvideo", "-pixel_format", "yuv420p"])
        .args(["-video_size", "64x64", "-framerate", "5", "-i"])
        .arg(&raw)
        .args(["-c:v", codec])
        .arg(directory.join(name))
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(made.success());
    std::fs::remove_file(raw).unwrap();
}

fn cli(arguments: &[&str]) -> (i32, String, String) {
    let output = Command::new(env!("CARGO_BIN_EXE_fotogramma-cli"))
        .args(arguments)
        .output()
        .unwrap();
    (
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

fn library(name: &str) -> Option<PathBuf> {
    let (Ok(ffmpeg), Ok(_)) = (ffmpeg_path(), ffprobe_path()) else {
        eprintln!("skipped: ffmpeg and ffprobe are not available");
        return None;
    };
    let root = std::env::temp_dir().join(format!("foto-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Show")).unwrap();
    // mpeg4 is "to convert". The minimal FFmpeg has no HEVC clip to offer
    // quickly, so the library is two files to convert and a broken one.
    make_clip(&ffmpeg, &root, "a.mp4", "mpeg4");
    make_clip(&ffmpeg, &root.join("Show"), "b.mp4", "mpeg4");
    std::fs::write(root.join("broken.mkv"), b"not a video").unwrap();
    Some(root)
}

#[test]
fn it_lists_what_is_left_to_convert() {
    let Some(root) = library("lists") else { return };
    let root_text = root.to_string_lossy().into_owned();
    let (code, stdout, stderr) = cli(&["scan", &root_text, "--no-cache"]);
    assert_eq!(code, 0, "{stderr}");
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(
        lines.len(),
        2,
        "two videos to convert, the broken one is not listed: {stdout}"
    );
    assert!(lines.iter().all(|line| line.ends_with(".mp4")));
    assert!(
        stderr.contains("3"),
        "the summary counts three videos: {stderr}"
    );

    let (_, quiet_out, quiet_err) = cli(&["scan", &root_text, "--no-cache", "-q"]);
    assert_eq!(quiet_out, stdout);
    assert!(quiet_err.is_empty(), "quiet means quiet: {quiet_err}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn csv_and_json_carry_the_details_and_everything_can_be_listed() {
    let Some(root) = library("formats") else {
        return;
    };
    let root_text = root.to_string_lossy().into_owned();
    let (_, csv, _) = cli(&[
        "scan",
        &root_text,
        "--no-cache",
        "-q",
        "-f",
        "csv",
        "--only",
        "all",
    ]);
    let rows: Vec<&str> = csv.lines().collect();
    assert!(rows[0].starts_with("path,folder,codec,category"));
    assert_eq!(rows.len(), 4, "a header and three videos: {csv}");
    assert!(csv.contains("mpeg4") && csv.contains(",error,") || csv.contains("error"));

    let (_, json, _) = cli(&[
        "scan",
        &root_text,
        "--no-cache",
        "-q",
        "-f",
        "json",
        "--only",
        "error",
    ]);
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.as_array().unwrap().len(), 1);
    assert_eq!(parsed[0]["category"], "error");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn accepted_codecs_change_what_is_left_and_the_exit_code_can_report_it() {
    let Some(root) = library("codecs") else {
        return;
    };
    let root_text = root.to_string_lossy().into_owned();
    // Accept mpeg4: nothing is left to convert.
    let (code, stdout, _) = cli(&[
        "scan",
        &root_text,
        "--no-cache",
        "-q",
        "--codecs",
        "mpeg4",
        "--fail-if-convert",
    ]);
    assert_eq!(code, 0);
    assert!(stdout.trim().is_empty());
    // Default codecs: two are left, and the exit code says so.
    let (code, _, _) = cli(&["scan", &root_text, "--no-cache", "-q", "--fail-if-convert"]);
    assert_eq!(code, 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_cache_file_is_written_and_reused_and_output_can_go_to_a_file() {
    let Some(root) = library("cache") else { return };
    let root_text = root.to_string_lossy().into_owned();
    let cache = root.join("cache.json");
    let cache_text = cache.to_string_lossy().into_owned();
    let list = root.join("list.txt");
    let list_text = list.to_string_lossy().into_owned();
    let (_, _, first) = cli(&["scan", &root_text, "--cache", &cache_text, "-o", &list_text]);
    assert!(cache.exists());
    assert!(
        first.contains("from the cache: 0") || first.contains("dalla cache: 0"),
        "{first}"
    );
    assert_eq!(std::fs::read_to_string(&list).unwrap().lines().count(), 2);
    let (_, _, second) = cli(&["scan", &root_text, "--cache", &cache_text, "-o", &list_text]);
    assert!(
        second.contains("from the cache: 2") || second.contains("dalla cache: 2"),
        "{second}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn usage_errors_exit_with_2_and_help_and_version_work() {
    let (code, _, stderr) = cli(&["scan"]);
    assert_eq!(code, 2);
    assert!(stderr.contains("fotogramma-cli"));
    let (code, _, _) = cli(&["scan", "/definitely/not/a/folder"]);
    assert_eq!(code, 2);
    let (code, stdout, _) = cli(&["--version"]);
    assert_eq!(code, 0);
    assert!(stdout.starts_with("fotogramma-cli "));
    let (code, stdout, _) = cli(&["--help"]);
    assert_eq!(code, 0);
    assert!(stdout.contains("--fail-if-convert"));
}
