//! Background scan and conversion workers. No UI calls happen on these threads:
//! every update travels through a channel to the GUI thread.

use eframe::egui;
use fotogramma_core::conversion::{
    allocate_output_path, allocate_partial_path, build_ffmpeg_args, ConversionProfile,
};
use fotogramma_core::media_tools::{ffmpeg_path, ffprobe_path, prepare_command};
use fotogramma_core::scanner::{discover_videos, probe_videos, DiscoveryWarning, VideoInfo};
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
                format!("Impossibile avviare ffmpeg: {error}"),
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
            message: "Conversione interrotta".to_string(),
            cancelled: true,
            verified: false,
            replaced: false,
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
            format!(
                "ffmpeg è terminato con codice {}",
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
                "File di output non creato".to_string(),
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
                format!("Originale non modificato: {validation_message}"),
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
    }
}
