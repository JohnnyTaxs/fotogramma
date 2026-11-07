//! Discovery and metadata scanning, including ffprobe process control.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub const VIDEO_EXTENSIONS: [&str; 3] = ["mp4", "mkv", "webm"];
pub const COMPATIBLE_CODECS: [&str; 2] = ["hevc", "av1"];
pub const PROBE_TIMEOUT_SECONDS: u64 = 45;

/// Classification follows the original application: a readable video is
/// compatible only when its first stream is HEVC or AV1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    Convert,
    Compatible,
    Error,
}

impl Category {
    pub fn as_str(self) -> &'static str {
        match self {
            Category::Convert => "convert",
            Category::Compatible => "compatible",
            Category::Error => "error",
        }
    }

    pub fn order(self) -> u8 {
        match self {
            Category::Convert => 0,
            Category::Compatible => 1,
            Category::Error => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VideoInfo {
    pub path: PathBuf,
    pub relative_path: String,
    pub name: String,
    pub folder: String,
    pub extension: String,
    pub category: Category,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl VideoInfo {
    pub fn status_label(&self) -> &'static str {
        match self.category {
            Category::Convert => "Converti in H.265",
            Category::Compatible => "Pronto",
            Category::Error => "Verifica file",
        }
    }

    pub fn folder_label(&self) -> &str {
        if self.folder.is_empty() {
            "Cartella principale"
        } else {
            &self.folder
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeError {
    Cancelled,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Summary {
    pub total: usize,
    pub to_convert: usize,
    pub compatible: usize,
    pub errors: usize,
    pub total_bytes: u64,
    pub to_convert_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveryWarning {
    pub path: PathBuf,
    pub message: String,
}

/// Recursively discover videos without following symbolic links.
/// Directory read failures are reported and the scan continues, unless the
/// root directory itself cannot be read.
pub fn discover_videos(
    directory: &Path,
    cancel: Option<&AtomicBool>,
    mut on_progress: impl FnMut(usize, &str),
    mut on_error: impl FnMut(&Path, &str),
) -> Result<Vec<PathBuf>, String> {
    let root = fs::canonicalize(directory).unwrap_or_else(|_| directory.to_path_buf());
    let mut candidates = Vec::new();
    walk(
        directory,
        &root,
        cancel,
        &mut candidates,
        &mut on_progress,
        &mut on_error,
    )?;
    Ok(candidates)
}

fn walk(
    directory: &Path,
    root: &Path,
    cancel: Option<&AtomicBool>,
    candidates: &mut Vec<PathBuf>,
    on_progress: &mut impl FnMut(usize, &str),
    on_error: &mut impl FnMut(&Path, &str),
) -> Result<(), String> {
    if is_cancelled(cancel) {
        return Ok(());
    }
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => {
            on_error(directory, &error.to_string());
            if same_path(directory, root) {
                return Err(format!("{}: {error}", directory.display()));
            }
            return Ok(());
        }
    };

    let mut directories = Vec::new();
    let mut files = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                on_error(directory, &error.to_string());
                continue;
            }
        };
        let path = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(_) => continue,
        };
        if file_type.is_dir() {
            directories.push(path);
        } else if path.is_file() {
            files.push(path);
        }
    }

    directories.sort_by_key(|path| path.to_string_lossy().to_lowercase());
    files.sort_by_key(|path| path.to_string_lossy().to_lowercase());

    for path in directories {
        if is_cancelled(cancel) {
            return Ok(());
        }
        walk(&path, root, cancel, candidates, on_progress, on_error)?;
    }

    for path in files {
        if is_cancelled(cancel) {
            return Ok(());
        }
        let extension = path
            .extension()
            .map(|value| value.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if !VIDEO_EXTENSIONS.contains(&extension.as_str()) {
            continue;
        }
        candidates.push(path.clone());
        let relative = relative_path(&path, root);
        on_progress(candidates.len(), &relative);
    }
    Ok(())
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right || fs::canonicalize(left).ok() == fs::canonicalize(right).ok()
}

fn is_cancelled(cancel: Option<&AtomicBool>) -> bool {
    cancel
        .map(|flag| flag.load(Ordering::Relaxed))
        .unwrap_or(false)
}

pub fn relative_path(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn parent_label(relative: &str) -> String {
    match Path::new(relative).parent() {
        Some(parent) if !parent.as_os_str().is_empty() => {
            parent.to_string_lossy().replace('\\', "/")
        }
        _ => String::new(),
    }
}

#[cfg(test)]
fn file_stem(path: &Path) -> String {
    path.file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn extension_of(path: &Path) -> String {
    path.extension()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn error_video(path: &Path, root: &Path, message: String, size: Option<u64>) -> VideoInfo {
    let relative = relative_path(path, root);
    VideoInfo {
        path: absolute(path),
        relative_path: relative.clone(),
        name: file_name(path),
        folder: parent_label(&relative),
        extension: extension_of(path),
        category: Category::Error,
        codec: None,
        size,
        duration: None,
        width: None,
        height: None,
        error: Some(message),
    }
}

fn absolute(path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    }
}

/// Read the first video stream (`v:0`) and stop ffprobe promptly on cancellation.
pub fn probe_video(
    file_path: &Path,
    root: &Path,
    ffprobe: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<VideoInfo, ProbeError> {
    let size = fs::metadata(file_path).ok().map(|metadata| metadata.len());
    if is_cancelled(cancel) {
        return Err(ProbeError::Cancelled);
    }

    let mut command = process_command(ffprobe);
    command
        .arg("-v")
        .arg("error")
        .arg("-select_streams")
        .arg("v:0")
        .arg("-show_entries")
        .arg("stream=codec_name,width,height:format=duration,size")
        .arg("-of")
        .arg("json")
        .arg(file_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return Ok(error_video(file_path, root, error.to_string(), size));
        }
    };

    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buffer);
        buffer
    });
    let stderr_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buffer);
        buffer
    });

    let deadline = Instant::now() + Duration::from_secs(PROBE_TIMEOUT_SECONDS);
    let status = loop {
        if is_cancelled(cancel) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Err(ProbeError::Cancelled);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = stdout_reader.join();
            let _ = stderr_reader.join();
            return Ok(error_video(
                file_path,
                root,
                format!("ffprobe non ha risposto entro {PROBE_TIMEOUT_SECONDS} secondi."),
                size,
            ));
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                return Ok(error_video(file_path, root, error.to_string(), size));
            }
        }
    };

    let stdout = String::from_utf8_lossy(&stdout_reader.join().unwrap_or_default()).into_owned();
    let stderr = String::from_utf8_lossy(&stderr_reader.join().unwrap_or_default()).into_owned();

    if !status.success() {
        let details = if stderr.trim().is_empty() {
            if stdout.trim().is_empty() {
                "File non leggibile da ffprobe.".to_string()
            } else {
                stdout.trim().to_string()
            }
        } else {
            stderr.trim().to_string()
        };
        let last = details.lines().last().unwrap_or_default();
        let last = truncate_chars(last.trim(), 400);
        return Ok(error_video(file_path, root, last, size));
    }

    let metadata: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(value) => value,
        Err(error) => {
            return Ok(error_video(file_path, root, error.to_string(), size));
        }
    };

    let streams = metadata
        .get("streams")
        .and_then(|value| value.as_array())
        .cloned()
        .unwrap_or_default();
    if streams.is_empty() {
        return Ok(error_video(
            file_path,
            root,
            "Nessun flusso video trovato.".to_string(),
            size,
        ));
    }
    let stream = &streams[0];
    let codec = stream
        .get("codec_name")
        .and_then(|value| value.as_str())
        .map(|value| value.to_lowercase())
        .filter(|value| !value.is_empty());
    let Some(codec) = codec else {
        return Ok(error_video(
            file_path,
            root,
            "Codec video non disponibile.".to_string(),
            size,
        ));
    };

    let relative = relative_path(file_path, root);
    let format = metadata.get("format");
    let duration = format
        .and_then(|value| value.get("duration"))
        .and_then(number);
    let format_size = format.and_then(|value| value.get("size")).and_then(integer);
    let category = if COMPATIBLE_CODECS.contains(&codec.as_str()) {
        Category::Compatible
    } else {
        Category::Convert
    };

    Ok(VideoInfo {
        path: absolute(file_path),
        relative_path: relative.clone(),
        name: file_name(file_path),
        folder: parent_label(&relative),
        extension: extension_of(file_path),
        category,
        codec: Some(codec),
        size: format_size.map(u64::from).or(size),
        duration,
        width: stream.get("width").and_then(integer),
        height: stream.get("height").and_then(integer),
        error: None,
    })
}

/// Probe a batch of candidates at a time so cancellation never leaves a long queue.
/// Results keep discovery order and are finally sorted by relative path.
pub fn probe_videos(
    candidates: &[PathBuf],
    root: &Path,
    ffprobe: &Path,
    cancel: Option<&AtomicBool>,
    mut on_progress: impl FnMut(usize, usize, &str, &VideoInfo),
) -> (Vec<VideoInfo>, bool) {
    let worker_count = std::thread::available_parallelism()
        .map(|value| value.get())
        .unwrap_or(2)
        .clamp(1, 4);
    let mut results = Vec::with_capacity(candidates.len());
    let mut processed = 0;
    let mut cancelled = false;

    for batch in candidates.chunks(worker_count) {
        if is_cancelled(cancel) {
            cancelled = true;
            break;
        }
        let batch_results: Vec<Result<VideoInfo, ProbeError>> = std::thread::scope(|scope| {
            let handles: Vec<_> = batch
                .iter()
                .map(|path| scope.spawn(move || probe_video(path, root, ffprobe, cancel)))
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap_or(Err(ProbeError::Cancelled)))
                .collect()
        });

        for (path, result) in batch.iter().zip(batch_results) {
            if let Ok(info) = result {
                processed += 1;
                on_progress(
                    processed,
                    candidates.len(),
                    &relative_path(path, root),
                    &info,
                );
                results.push(info);
            } else {
                cancelled = true;
            }
        }
        if cancelled {
            break;
        }
    }

    results.sort_by(|left, right| {
        left.relative_path
            .to_lowercase()
            .cmp(&right.relative_path.to_lowercase())
    });
    let cancelled = cancelled || is_cancelled(cancel);
    (results, cancelled)
}

pub fn summarize(videos: &[VideoInfo], total: Option<usize>) -> Summary {
    let mut summary = Summary {
        total: total.unwrap_or(videos.len()),
        ..Summary::default()
    };
    for video in videos {
        match video.category {
            Category::Convert => {
                summary.to_convert += 1;
                summary.to_convert_bytes += video.size.unwrap_or(0);
            }
            Category::Compatible => summary.compatible += 1,
            Category::Error => summary.errors += 1,
        }
        summary.total_bytes += video.size.unwrap_or(0);
    }
    summary
}

pub fn stream_categories(json: &str) -> Result<BTreeMap<String, usize>, String> {
    let metadata: serde_json::Value =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    let mut counts = BTreeMap::new();
    for stream in metadata
        .get("streams")
        .and_then(|value| value.as_array())
        .into_iter()
        .flatten()
    {
        let kind = stream
            .get("codec_type")
            .and_then(|value| value.as_str())
            .unwrap_or("unknown");
        *counts.entry(kind.to_string()).or_insert(0) += 1;
    }
    Ok(counts)
}

fn number(value: &serde_json::Value) -> Option<f64> {
    let parsed = match value {
        serde_json::Value::Number(number) => number.as_f64(),
        serde_json::Value::String(text) => text.parse::<f64>().ok(),
        _ => None,
    }?;
    if parsed.is_finite() && parsed >= 0.0 {
        Some(parsed)
    } else {
        None
    }
}

fn integer(value: &serde_json::Value) -> Option<u32> {
    number(value).map(|value| value as u32)
}

fn truncate_chars(value: &str, limit: usize) -> String {
    value.chars().take(limit).collect()
}

/// Spawn a command without a console window on Windows.
pub fn process_command(program: &Path) -> Command {
    #[allow(unused_mut)]
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_compatible_codecs() {
        for codec in COMPATIBLE_CODECS {
            assert!(codec == "hevc" || codec == "av1");
        }
    }

    #[test]
    fn category_labels_match_the_interface() {
        assert_eq!(Category::Convert.as_str(), "convert");
        assert_eq!(Category::Compatible.order(), 1);
        assert_eq!(Category::Error.order(), 2);
    }

    #[test]
    fn summarizes_categories_and_bytes() {
        let base = VideoInfo {
            path: PathBuf::from("/videos/a.mp4"),
            relative_path: "a.mp4".into(),
            name: "a.mp4".into(),
            folder: String::new(),
            extension: "mp4".into(),
            category: Category::Convert,
            codec: Some("h264".into()),
            size: Some(100),
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            error: None,
        };
        let mut compatible = base.clone();
        compatible.category = Category::Compatible;
        compatible.size = Some(50);
        let mut error = base.clone();
        error.category = Category::Error;
        error.size = None;

        let summary = summarize(&[base, compatible, error], None);
        assert_eq!(summary.total, 3);
        assert_eq!(summary.to_convert, 1);
        assert_eq!(summary.compatible, 1);
        assert_eq!(summary.errors, 1);
        assert_eq!(summary.to_convert_bytes, 100);
        assert_eq!(summary.total_bytes, 150);
    }

    #[test]
    fn parses_stream_categories() {
        let json =
            r#"{"streams":[{"codec_type":"video"},{"codec_type":"audio"},{"codec_type":"audio"}]}"#;
        let counts = stream_categories(json).unwrap();
        assert_eq!(counts["video"], 1);
        assert_eq!(counts["audio"], 2);
    }

    #[test]
    fn derives_folder_and_name() {
        let path = Path::new("/root/Vacanze/Tramonto.mkv");
        let relative = relative_path(path, Path::new("/root"));
        assert_eq!(relative, "Vacanze/Tramonto.mkv");
        assert_eq!(parent_label(&relative), "Vacanze");
        assert_eq!(extension_of(path), "mkv");
        assert_eq!(file_stem(path), "Tramonto");
    }
}
