//! Background scan and conversion workers. No UI calls happen on these threads:
//! every update travels through a channel to the GUI thread.

use eframe::egui;
use fotogramma_core::conversion::{
    allocate_output_path, allocate_partial_path, build_ffmpeg_args, ConversionProfile,
};
use fotogramma_core::i18n::tr_str;
use fotogramma_core::media_tools::{ffmpeg_path, ffprobe_path, prepare_command};
use fotogramma_core::scanner::{discover_videos, probe_videos, DiscoveryWarning, VideoInfo};
use fotogramma_core::update::{self, InstallTarget, Release};
use fotogramma_core::verification::{replace_verified_output, validate_conversion_output};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Clone, Debug)]
pub enum ScanMessage {
    Progress {
        processed: usize,
        total: Option<usize>,
        current: String,
    },
    Completed {
        videos: Vec<VideoInfo>,
        warnings: Vec<DiscoveryWarning>,
        cancelled: bool,
    },
    Failed(String),
}

pub fn spawn_scan(
    directory: PathBuf,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
) -> Receiver<ScanMessage> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        if let Err(message) = run_scan(&directory, &cancel, &sender, &ctx) {
            let _ = sender.send(ScanMessage::Failed(message));
        }
        ctx.request_repaint();
    });
    receiver
}

fn run_scan(
    directory: &std::path::Path,
    cancel: &Arc<AtomicBool>,
    sender: &Sender<ScanMessage>,
    ctx: &egui::Context,
) -> Result<(), String> {
    let ffprobe = ffprobe_path()?;
    let mut warnings = Vec::new();

    let mut discovered = |found: usize, current: &str| {
        let _ = sender.send(ScanMessage::Progress {
            processed: found,
            total: None,
            current: current.to_string(),
        });
        ctx.request_repaint();
    };
    let mut discovery_error = |path: &std::path::Path, message: &str| {
        warnings.push(DiscoveryWarning {
            path: path.to_path_buf(),
            message: message.to_string(),
        });
    };
    let candidates = discover_videos(
        directory,
        Some(cancel),
        &mut discovered,
        &mut discovery_error,
    )?;

    if cancel.load(Ordering::Relaxed) {
        let _ = sender.send(ScanMessage::Completed {
            videos: Vec::new(),
            warnings,
            cancelled: true,
        });
        return Ok(());
    }

    let mut progressed = |processed: usize, total: usize, current: &str, _info: &VideoInfo| {
        let _ = sender.send(ScanMessage::Progress {
            processed,
            total: Some(total),
            current: current.to_string(),
        });
        ctx.request_repaint();
    };
    let (videos, cancelled) = probe_videos(
        &candidates,
        directory,
        &ffprobe,
        Some(cancel),
        &mut progressed,
    );

    let _ = sender.send(ScanMessage::Completed {
        videos,
        warnings,
        cancelled,
    });
    Ok(())
}

#[derive(Clone, Debug)]
pub struct ConversionResult {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub success: bool,
    pub message: String,
    pub cancelled: bool,
    pub verified: bool,
    pub replaced: bool,
    /// Size of the original before the conversion, and of the result after it.
    pub original_size: Option<u64>,
    pub new_size: Option<u64>,
}

fn file_size(path: &std::path::Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|metadata| metadata.len())
}

impl ConversionResult {
    fn failure(source: &std::path::Path, destination: &std::path::Path, message: String) -> Self {
        Self {
            source: source.to_path_buf(),
            destination: destination.to_path_buf(),
            success: false,
            message,
            cancelled: false,
            verified: false,
            replaced: false,
            original_size: None,
            new_size: None,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ConversionMessage {
    FileStarted {
        index: usize,
        total: usize,
        source: PathBuf,
        destination: PathBuf,
    },
    Progress {
        index: usize,
        total: usize,
        percent: u8,
        current_file: PathBuf,
    },
    VerificationStarted {
        index: usize,
        current_file: PathBuf,
    },
    FileCompleted(Box<ConversionResult>),
    Completed(Vec<ConversionResult>),
    Failed(String),
}

pub fn spawn_conversion(
    videos: Vec<VideoInfo>,
    profile: ConversionProfile,
    destination_dir: PathBuf,
    container: String,
    replace_original: bool,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
) -> Receiver<ConversionMessage> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        if let Err(message) = run_conversion(
            videos,
            profile,
            destination_dir,
            container,
            replace_original,
            cancel,
            &sender,
            &ctx,
        ) {
            let _ = sender.send(ConversionMessage::Failed(message));
        }
        ctx.request_repaint();
    });
    receiver
}

#[allow(clippy::too_many_arguments)]
fn run_conversion(
    videos: Vec<VideoInfo>,
    profile: ConversionProfile,
    destination_dir: PathBuf,
    container: String,
    replace_original: bool,
    cancel: Arc<AtomicBool>,
    sender: &Sender<ConversionMessage>,
    ctx: &egui::Context,
) -> Result<(), String> {
    let ffmpeg = ffmpeg_path()?;
    if !replace_original {
        std::fs::create_dir_all(&destination_dir).map_err(|error| error.to_string())?;
    }
    let mut results = Vec::new();
    let total = videos.len();

    for (index, video) in videos.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let destination = if replace_original {
            match allocate_partial_path(&video.path) {
                Ok(path) => path,
                Err(error) => {
                    let result = ConversionResult::failure(&video.path, &video.path, error);
                    let _ = sender.send(ConversionMessage::FileCompleted(Box::new(result)));
                    ctx.request_repaint();
                    continue;
                }
            }
        } else {
            match allocate_output_path(&video.path, &destination_dir, &container) {
                Ok(path) => path,
                Err(error) => {
                    let result = ConversionResult::failure(&video.path, &video.path, error);
                    let _ = sender.send(ConversionMessage::FileCompleted(Box::new(result)));
                    ctx.request_repaint();
                    continue;
                }
            }
        };

        let _ = sender.send(ConversionMessage::FileStarted {
            index: index + 1,
            total,
            source: video.path.clone(),
            destination: destination.clone(),
        });
        ctx.request_repaint();

        let result = convert_one(
            &ffmpeg,
            &profile,
            video,
            &destination,
            index + 1,
            total,
            replace_original,
            &cancel,
            sender,
            ctx,
        );
        let _ = sender.send(ConversionMessage::FileCompleted(Box::new(result.clone())));
        ctx.request_repaint();
        results.push(result);
    }

    let _ = sender.send(ConversionMessage::Completed(results));
    ctx.request_repaint();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn convert_one(
    ffmpeg: &std::path::Path,
    profile: &ConversionProfile,
    video: &VideoInfo,
    destination: &std::path::Path,
    file_index: usize,
    total: usize,
    replace_original: bool,
    cancel: &Arc<AtomicBool>,
    sender: &Sender<ConversionMessage>,
    ctx: &egui::Context,
) -> ConversionResult {
    // Read before anything can replace the original.
    let original_size = file_size(&video.path);
    let mut command = prepare_command(ffmpeg);
    command
        .args(build_ffmpeg_args(profile, &video.path, destination))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) => {
            return ConversionResult::failure(
                &video.path,
                destination,
                fotogramma_core::tr!(
                    "Impossibile avviare ffmpeg: {error}",
                    "Cannot start ffmpeg: {error}"
                ),
            );
        }
    };

    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let (line_sender, line_receiver) = mpsc::channel::<String>();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if line_sender.send(line).is_err() {
                break;
            }
        }
    });
    let error_reader = thread::spawn(move || {
        BufReader::new(stderr)
            .lines()
            .map_while(Result::ok)
            .collect::<Vec<String>>()
    });

    let mut log: VecDeque<String> = VecDeque::new();
    let mut cancelled = false;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            cancelled = true;
            break;
        }
        match line_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                let line = line.trim().to_string();
                if let Some(value) = line.strip_prefix("out_time_ms=") {
                    if let Some(duration) = video.duration.filter(|value| *value > 0.0) {
                        let elapsed = value.parse::<f64>().unwrap_or(0.0) / 1_000_000.0;
                        let percent = ((elapsed / duration) * 100.0).round().clamp(0.0, 99.0) as u8;
                        let _ = sender.send(ConversionMessage::Progress {
                            index: file_index,
                            total,
                            percent,
                            current_file: video.path.clone(),
                        });
                        ctx.request_repaint();
                    }
                } else if line == "progress=end" {
                    let _ = sender.send(ConversionMessage::Progress {
                        index: file_index,
                        total,
                        percent: 100,
                        current_file: video.path.clone(),
                    });
                    ctx.request_repaint();
                } else if !line.is_empty() {
                    log.push_back(line);
                    if log.len() > 8 {
                        log.pop_front();
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    let status = child.wait();
    let _ = reader.join();
    let stderr_lines = error_reader.join().unwrap_or_default();
    for line in stderr_lines.into_iter().rev().take(8).rev() {
        if !line.trim().is_empty() {
            log.push_back(line);
            if log.len() > 8 {
                log.pop_front();
            }
        }
    }

    if cancelled || cancel.load(Ordering::Relaxed) {
        let _ = std::fs::remove_file(destination);
        return ConversionResult {
            source: video.path.clone(),
            destination: destination.to_path_buf(),
            success: false,
            message: tr_str("Conversione interrotta", "Conversion stopped").to_string(),
            cancelled: true,
            verified: false,
            replaced: false,
            original_size: None,
            new_size: None,
        };
    }

    let status = match status {
        Ok(status) => status,
        Err(error) => {
            return ConversionResult::failure(&video.path, destination, error.to_string());
        }
    };
    if !status.success() {
        let message = if log.is_empty() {
            fotogramma_core::tr!(
                "ffmpeg è terminato con codice {}",
                "ffmpeg exited with code {}",
                status.code().unwrap_or(-1)
            )
        } else {
            log.iter().cloned().collect::<Vec<_>>().join("\n")
        };
        return ConversionResult::failure(&video.path, destination, message);
    }

    match std::fs::metadata(destination) {
        Ok(metadata) if metadata.len() > 0 => {}
        _ => {
            return ConversionResult::failure(
                &video.path,
                destination,
                tr_str("File di output non creato", "Output file not created").to_string(),
            );
        }
    }

    if replace_original {
        let _ = sender.send(ConversionMessage::VerificationStarted {
            index: file_index,
            current_file: video.path.clone(),
        });
        ctx.request_repaint();
        let ffprobe = match ffprobe_path() {
            Ok(path) => path,
            Err(error) => {
                return ConversionResult::failure(&video.path, destination, error);
            }
        };
        let (valid, validation_message) =
            validate_conversion_output(video, destination, &ffprobe, ffmpeg);
        if !valid {
            let _ = std::fs::remove_file(destination);
            return ConversionResult::failure(
                &video.path,
                destination,
                fotogramma_core::tr!(
                    "Originale non modificato: {validation_message}",
                    "Original left unchanged: {validation_message}"
                ),
            );
        }
        let (replaced, replacement_message) =
            replace_verified_output(&video.path, destination, &ffprobe);
        if !replaced {
            let _ = std::fs::remove_file(destination);
            return ConversionResult::failure(&video.path, destination, replacement_message);
        }
        return ConversionResult {
            source: video.path.clone(),
            destination: video.path.clone(),
            success: true,
            message: replacement_message,
            cancelled: false,
            verified: true,
            replaced: true,
            original_size,
            new_size: file_size(&video.path),
        };
    }

    ConversionResult {
        source: video.path.clone(),
        destination: destination.to_path_buf(),
        success: true,
        message: String::new(),
        cancelled: false,
        verified: false,
        replaced: false,
        original_size,
        new_size: file_size(destination),
    }
}

pub enum CheckMessage {
    Available(Box<Release>),
    UpToDate,
    Failed(String),
}

/// Ask GitHub whether a newer release exists. Runs once, off the GUI thread.
pub fn spawn_update_check(current: &'static str, ctx: egui::Context) -> Receiver<CheckMessage> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let message = match update::check(current) {
            Ok(Some(release)) => CheckMessage::Available(Box::new(release)),
            Ok(None) => CheckMessage::UpToDate,
            Err(error) => CheckMessage::Failed(error),
        };
        let _ = sender.send(message);
        ctx.request_repaint();
    });
    receiver
}

pub enum InstallMessage {
    Progress {
        done: u64,
        total: u64,
    },
    /// The new copy is unpacked and verified, waiting for the restart.
    Staged(std::path::PathBuf),
    Failed(String),
}

/// Download, verify and unpack a release next to the installed copy.
pub fn spawn_update_install(
    release: Release,
    target: InstallTarget,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
) -> Receiver<InstallMessage> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let directory =
            std::env::temp_dir().join(format!("fotogramma-update-{}", std::process::id()));
        let mut last_percent = u64::MAX;
        let mut report = |done: u64, total: u64| {
            let percent = (done * 100).checked_div(total).unwrap_or(0);
            if percent != last_percent {
                last_percent = percent;
                let _ = sender.send(InstallMessage::Progress { done, total });
                ctx.request_repaint();
            }
        };
        let result = update::download(&release, &directory, &cancel, &mut report)
            .and_then(|archive| update::stage(&archive, &target));
        let _ = std::fs::remove_dir_all(&directory);
        let _ = sender.send(match result {
            Ok(staged) => InstallMessage::Staged(staged),
            Err(error) => InstallMessage::Failed(error),
        });
        ctx.request_repaint();
    });
    receiver
}

#[cfg(test)]
mod tests {
    use super::*;
    use fotogramma_core::conversion::builtin_profiles;
    use fotogramma_core::scanner::{probe_video, Category};
    use std::process::Stdio;
    use std::time::Duration;

    /// Converts a real clip and checks that both sizes reach the result.
    /// Skipped when FFmpeg is not installed.
    #[test]
    fn a_real_conversion_reports_both_sizes() {
        let (Ok(ffmpeg), Ok(ffprobe)) = (ffmpeg_path(), ffprobe_path()) else {
            eprintln!("skipped: ffmpeg and ffprobe are not available");
            return;
        };
        let directory = std::env::temp_dir().join(format!("foto-size-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let raw = directory.join("source.yuv");
        std::fs::write(&raw, vec![0u8; 64 * 64 * 3 / 2 * 10]).unwrap();
        let source = directory.join("clip.mp4");
        let made = prepare_command(&ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args(["-f", "rawvideo", "-pixel_format", "yuv420p"])
            .args(["-video_size", "64x64", "-framerate", "5", "-i"])
            .arg(&raw)
            .args(["-c:v", "mpeg4"])
            .arg(&source)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(made.success());

        let video = probe_video(&source, &directory, &ffprobe, None).unwrap();
        assert_eq!(video.category, Category::Convert);
        let profile = builtin_profiles()
            .into_iter()
            .find(|p| p.key == "fast")
            .unwrap();
        let receiver = spawn_conversion(
            vec![video],
            profile,
            directory.join("out"),
            "mkv".to_string(),
            false,
            Arc::new(AtomicBool::new(false)),
            egui::Context::default(),
        );
        let mut results = None;
        while let Ok(message) = receiver.recv_timeout(Duration::from_secs(120)) {
            if let ConversionMessage::Completed(done) = message {
                results = Some(done);
                break;
            }
        }
        let results = results.expect("the conversion finishes");
        let result = &results[0];
        assert!(result.success, "{}", result.message);
        let before = std::fs::metadata(&source).unwrap().len();
        let after = std::fs::metadata(&result.destination).unwrap().len();
        assert_eq!(result.original_size, Some(before));
        assert_eq!(result.new_size, Some(after));

        // Replacing the original: the size before is read before the swap.
        let second = directory.join("second.mp4");
        let made = prepare_command(&ffmpeg)
            .args(["-y", "-hide_banner", "-loglevel", "error"])
            .args(["-f", "rawvideo", "-pixel_format", "yuv420p"])
            .args(["-video_size", "64x64", "-framerate", "5", "-i"])
            .arg(&raw)
            .args(["-c:v", "mpeg4"])
            .arg(&second)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(made.success());
        let before = std::fs::metadata(&second).unwrap().len();
        let video = probe_video(&second, &directory, &ffprobe, None).unwrap();
        let profile = builtin_profiles()
            .into_iter()
            .find(|p| p.key == "fast")
            .unwrap();
        let receiver = spawn_conversion(
            vec![video],
            profile,
            directory.clone(),
            "mp4".to_string(),
            true,
            Arc::new(AtomicBool::new(false)),
            egui::Context::default(),
        );
        let mut results = None;
        while let Ok(message) = receiver.recv_timeout(Duration::from_secs(120)) {
            if let ConversionMessage::Completed(done) = message {
                results = Some(done);
                break;
            }
        }
        let results = results.expect("the replacement finishes");
        let result = &results[0];
        assert!(result.replaced, "{}", result.message);
        assert_eq!(result.original_size, Some(before));
        assert_eq!(
            result.new_size,
            std::fs::metadata(&second).ok().map(|m| m.len())
        );
        assert_ne!(
            result.new_size,
            Some(before),
            "the file was really replaced"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
