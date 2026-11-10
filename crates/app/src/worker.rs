//! Background scan and conversion workers. No UI calls happen on these threads:
//! every update travels through a channel to the GUI thread.

use eframe::egui;
use fotogramma_core::cache::ScanCache;
use fotogramma_core::conversion::{
    allocate_output_path, allocate_partial_path, build_ffmpeg_args, container_for, existing_output,
    output_dir_for, profile_for_video, ConversionProfile,
};
use fotogramma_core::formatting::SpaceChange;
use fotogramma_core::i18n::tr_str;
use fotogramma_core::media_tools::{ffmpeg_path, ffprobe_path, prepare_command};
use fotogramma_core::queue::PendingQueue;
use fotogramma_core::scanner::{discover_videos, probe_videos_cached, DiscoveryWarning, VideoInfo};
use fotogramma_core::update::{self, InstallTarget, Release};
use fotogramma_core::verification::{
    replace_verified_output, subtitle_codecs, validate_conversion_output,
};
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
        /// Results taken from the cache instead of `ffprobe`.
        from_cache: usize,
        /// Videos to convert that an earlier conversion already made a file for.
        already_converted: Vec<(PathBuf, PathBuf)>,
    },
    Failed(String),
}

/// What a scan is allowed to reuse.
#[derive(Clone, Debug, Default)]
pub struct ScanOptions {
    /// The cache file, or `None` to probe everything.
    pub cache_file: Option<PathBuf>,
    /// The output folder of the settings, to find earlier conversions.
    pub output_dir: String,
}

pub fn spawn_scan(
    directory: PathBuf,
    options: ScanOptions,
    cancel: Arc<AtomicBool>,
    ctx: egui::Context,
) -> Receiver<ScanMessage> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        if let Err(message) = run_scan(&directory, &options, &cancel, &sender, &ctx) {
            let _ = sender.send(ScanMessage::Failed(message));
        }
        ctx.request_repaint();
    });
    receiver
}

fn run_scan(
    directory: &std::path::Path,
    options: &ScanOptions,
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
            from_cache: 0,
            already_converted: Vec::new(),
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
    let cache = options.cache_file.as_deref().map(ScanCache::load);
    let outcome = probe_videos_cached(
        &candidates,
        directory,
        &ffprobe,
        Some(cancel),
        cache.as_ref(),
        &mut progressed,
    );
    if let (Some(cache), Some(file)) = (&cache, &options.cache_file) {
        cache.finish(directory, !outcome.cancelled);
        // A cache that cannot be written only costs speed next time.
        let _ = cache.save(file);
    }
    let already_converted = outcome
        .videos
        .iter()
        .filter(|video| video.category == fotogramma_core::scanner::Category::Convert)
        .filter_map(|video| {
            let folder = output_dir_for(&video.path, &options.output_dir);
            existing_output(&video.path, &folder).map(|output| (video.path.clone(), output))
        })
        .collect();

    let _ = sender.send(ScanMessage::Completed {
        videos: outcome.videos,
        warnings,
        cancelled: outcome.cancelled,
        from_cache: outcome.from_cache,
        already_converted,
    });
    Ok(())
}

/// What a conversion run is asked to do beyond the encoder settings.
#[derive(Clone, Debug, Default)]
pub struct ConversionOptions {
    /// Keep the original, and drop the result, when the new file saves less
    /// than this percentage. 0 keeps every result.
    pub min_saving_percent: u8,
    /// Stop a file that is clearly heading for a result too big.
    pub stop_if_larger: bool,
    /// Keep the computer awake while the queue runs.
    pub prevent_sleep: bool,
    /// Where to keep the queue while it runs, so it can be resumed.
    pub queue_file: Option<PathBuf>,
    /// While set, no new file starts (the one in progress finishes).
    pub pause: Arc<AtomicBool>,
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
    /// The result saved too little: it was dropped and the original kept.
    pub skipped: bool,
}

/// Give a converted file the modification date of the original, so media
/// servers that sort by date do not see the whole library as new.
fn restore_mtime(time: Option<std::time::SystemTime>, path: &std::path::Path) {
    let Some(time) = time else {
        return;
    };
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(time);
    }
}

/// Seconds left for `remaining_media` seconds of video, from how fast the
/// current file has been going. `None` until there is something to go on.
fn estimate_seconds_left(done_media: f64, wall_seconds: f64, remaining_media: f64) -> Option<u64> {
    if done_media <= 0.0 || wall_seconds < 2.0 {
        return None;
    }
    let speed = done_media / wall_seconds;
    Some((remaining_media / speed).round() as u64)
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
            skipped: false,
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
        /// Rough seconds left for the whole queue, once the speed is known.
        eta: Option<u64>,
    },
    VerificationStarted {
        index: usize,
        current_file: PathBuf,
    },
    FileCompleted(Box<ConversionResult>),
    Completed(Vec<ConversionResult>),
    Failed(String),
}

#[allow(clippy::too_many_arguments)]
pub fn spawn_conversion(
    videos: Vec<VideoInfo>,
    profile: ConversionProfile,
    destination_dir: PathBuf,
    container: String,
    replace_original: bool,
    options: ConversionOptions,
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
            options,
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
    options: ConversionOptions,
    cancel: Arc<AtomicBool>,
    sender: &Sender<ConversionMessage>,
    ctx: &egui::Context,
) -> Result<(), String> {
    let ffmpeg = ffmpeg_path()?;
    // Held until the queue ends, on this thread (Windows ties it to the thread).
    let _awake = crate::platform::SleepGuard::acquire(options.prevent_sleep);
    let ffprobe = ffprobe_path().ok();
    let queue_file = options.queue_file.clone();
    let save_queue = |remaining: &[VideoInfo]| {
        if let Some(file) = &queue_file {
            if remaining.is_empty() {
                PendingQueue::remove(file);
            } else {
                let queue = PendingQueue::new(
                    remaining.to_vec(),
                    profile.clone(),
                    destination_dir.clone(),
                    container.clone(),
                    replace_original,
                    options.min_saving_percent,
                );
                let _ = queue.save(file);
            }
        }
    };
    save_queue(&videos);
    if !replace_original {
        std::fs::create_dir_all(&destination_dir).map_err(|error| error.to_string())?;
    }
    let mut results = Vec::new();
    let total = videos.len();

    for (index, video) in videos.iter().enumerate() {
        // Paused: the file in progress was allowed to finish, the next waits.
        while options.pause.load(Ordering::Relaxed) && !cancel.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(200));
        }
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        // MP4 cannot hold most subtitles: such a file goes into MKV instead.
        let mut notes: Vec<String> = Vec::new();
        let mut file_container = container.clone();
        if !replace_original {
            let codecs = ffprobe
                .as_ref()
                .and_then(|path| subtitle_codecs(&video.path, path).ok())
                .unwrap_or_default();
            file_container = container_for(&container, &codecs).to_string();
            if file_container != container {
                notes.push(
                    tr_str(
                        "sottotitoli non adatti a MP4: salvato come MKV",
                        "subtitles do not fit MP4: saved as MKV",
                    )
                    .to_string(),
                );
            }
        }
        let file_profile = profile_for_video(&profile, video.hdr);
        if video.hdr && file_profile.pixel_format != profile.pixel_format {
            notes.push(tr_str("HDR: elaborato a 10 bit", "HDR: processed in 10 bit").to_string());
        }
        let remaining_seconds: f64 = videos[index + 1..]
            .iter()
            .map(|later| later.duration.unwrap_or(0.0))
            .sum();
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
            match allocate_output_path(&video.path, &destination_dir, &file_container) {
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

        let mut result = convert_one(
            &ffmpeg,
            &file_profile,
            video,
            &destination,
            (index + 1, total),
            replace_original,
            (
                options.min_saving_percent,
                remaining_seconds,
                options.stop_if_larger,
            ),
            &cancel,
            sender,
            ctx,
        );
        if result.success && !notes.is_empty() {
            result.message = notes.join(" · ");
        }
        let _ = sender.send(ConversionMessage::FileCompleted(Box::new(result.clone())));
        ctx.request_repaint();
        results.push(result);
        save_queue(&videos[index + 1..]);
    }
    // Finished or stopped on purpose: there is nothing to resume.
    save_queue(&[]);

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
    (file_index, total): (usize, usize),
    replace_original: bool,
    (min_saving_percent, remaining_seconds, stop_if_larger): (u8, f64, bool),
    cancel: &Arc<AtomicBool>,
    sender: &Sender<ConversionMessage>,
    ctx: &egui::Context,
) -> ConversionResult {
    // Read before anything can replace the original.
    let original_size = file_size(&video.path);
    let original_time = std::fs::metadata(&video.path)
        .and_then(|metadata| metadata.modified())
        .ok();
    let started = std::time::Instant::now();
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
    // (percent, projected bytes) when the file is stopped for being too big.
    let mut too_big: Option<(u8, u64)> = None;
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
                        let eta = estimate_seconds_left(
                            elapsed,
                            started.elapsed().as_secs_f64(),
                            (duration - elapsed).max(0.0) + remaining_seconds,
                        );
                        let _ = sender.send(ConversionMessage::Progress {
                            index: file_index,
                            total,
                            percent,
                            current_file: video.path.clone(),
                            eta,
                        });
                        ctx.request_repaint();
                        if stop_if_larger {
                            if let (Some(before), Some(now)) =
                                (original_size, file_size(destination))
                            {
                                if let Some(projected) =
                                    fotogramma_core::conversion::heading_for_too_big(
                                        before,
                                        now,
                                        elapsed / duration,
                                        min_saving_percent,
                                    )
                                {
                                    let _ = child.kill();
                                    too_big = Some((percent, projected));
                                    break;
                                }
                            }
                        }
                    }
                } else if line == "progress=end" {
                    let _ = sender.send(ConversionMessage::Progress {
                        index: file_index,
                        total,
                        percent: 100,
                        current_file: video.path.clone(),
                        eta: None,
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
            skipped: false,
        };
    }

    if let Some((percent, projected)) = too_big {
        let _ = std::fs::remove_file(destination);
        let before = original_size.unwrap_or(0);
        return ConversionResult {
            source: video.path.clone(),
            destination: destination.to_path_buf(),
            success: false,
            message: fotogramma_core::tr!(
                "fermato al {}%: il file finirebbe intorno a {} contro {} dell'originale. Originale tenuto. Prova un CRF più alto.",
                "stopped at {}%: the file would end around {} against {} for the original. Original kept. Try a higher CRF.",
                percent,
                fotogramma_core::formatting::format_bytes(Some(projected)),
                fotogramma_core::formatting::format_bytes(Some(before))
            ),
            cancelled: false,
            verified: false,
            replaced: false,
            original_size,
            new_size: None,
            skipped: true,
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

    // Not worth it: keep the original and drop the result.
    let new_size = file_size(destination);
    if let (true, Some(before), Some(after)) = (min_saving_percent > 0, original_size, new_size) {
        let limit = before as f64 * (100.0 - f64::from(min_saving_percent)) / 100.0;
        if after as f64 > limit {
            let saved = SpaceChange { before, after };
            let _ = std::fs::remove_file(destination);
            return ConversionResult {
                source: video.path.clone(),
                destination: destination.to_path_buf(),
                success: false,
                message: fotogramma_core::tr!(
                    "risparmio {} sotto la soglia del {}%: originale tenuto, risultato scartato",
                    "saving of {} is under the {}% threshold: original kept, result dropped",
                    saved.signed_percent(),
                    min_saving_percent
                ),
                cancelled: false,
                verified: false,
                replaced: false,
                original_size,
                new_size,
                skipped: true,
            };
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
        restore_mtime(original_time, &video.path);
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
            skipped: false,
        };
    }

    restore_mtime(original_time, destination);
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
        skipped: false,
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
            ConversionOptions::default(),
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
            ConversionOptions::default(),
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

    // ---- real conversions with the new options (skipped without FFmpeg) ----

    fn tools() -> Option<(PathBuf, PathBuf)> {
        Some((ffmpeg_path().ok()?, ffprobe_path().ok()?))
    }

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("foto-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    /// A tiny mpeg4 clip made with the tools under test.
    fn clip(ffmpeg: &std::path::Path, directory: &std::path::Path, name: &str) -> PathBuf {
        let raw = directory.join("raw.yuv");
        std::fs::write(&raw, vec![0u8; 64 * 64 * 3 / 2 * 10]).unwrap();
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
        path
    }

    fn convert(
        video: VideoInfo,
        profile_key: &str,
        container: &str,
        destination: PathBuf,
        replace: bool,
        options: ConversionOptions,
    ) -> ConversionResult {
        let profile = builtin_profiles()
            .into_iter()
            .find(|p| p.key == profile_key)
            .unwrap();
        let receiver = spawn_conversion(
            vec![video],
            profile,
            destination,
            container.to_string(),
            replace,
            options,
            Arc::new(AtomicBool::new(false)),
            egui::Context::default(),
        );
        while let Ok(message) = receiver.recv_timeout(Duration::from_secs(120)) {
            if let ConversionMessage::Completed(mut done) = message {
                return done.remove(0);
            }
        }
        panic!("the conversion did not finish");
    }

    #[test]
    fn a_file_with_subtitles_goes_into_mkv_when_mp4_was_asked_for() {
        let Some((_, ffprobe)) = tools() else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let fixture =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/subtitles.mkv");
        let directory = scratch("subs");
        let source = directory.join("subtitles.mkv");
        std::fs::copy(&fixture, &source).unwrap();
        let video = probe_video(&source, &directory, &ffprobe, None).unwrap();
        let result = convert(
            video,
            "fast",
            "mp4",
            directory.join("out"),
            false,
            ConversionOptions::default(),
        );
        assert!(result.success, "{}", result.message);
        assert_eq!(result.destination.extension().unwrap(), "mkv");
        assert!(result.message.contains("MKV"), "{}", result.message);
        let codecs = subtitle_codecs(&result.destination, &ffprobe).unwrap();
        assert_eq!(codecs, vec!["subrip"], "the subtitles are kept");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_result_that_saves_too_little_is_dropped_and_the_original_kept() {
        let Some((ffmpeg, ffprobe)) = tools() else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let directory = scratch("small");
        let source = clip(&ffmpeg, &directory, "a.mp4");
        let video = probe_video(&source, &directory, &ffprobe, None).unwrap();
        let out = directory.join("out");
        // 99% is a threshold no encode of a blank clip reaches.
        let result = convert(
            video,
            "fast",
            "mkv",
            out.clone(),
            false,
            ConversionOptions {
                min_saving_percent: 99,
                ..ConversionOptions::default()
            },
        );
        assert!(result.skipped && !result.success, "{}", result.message);
        assert!(source.exists(), "the original is untouched");
        assert!(
            std::fs::read_dir(&out)
                .map(|mut d| d.next().is_none())
                .unwrap_or(true),
            "the dropped result is deleted"
        );
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_file_that_cannot_save_enough_is_dropped_with_the_early_stop_on() {
        let Some((ffmpeg, ffprobe)) = tools() else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let directory = scratch("early");
        let source = clip(&ffmpeg, &directory, "a.mp4");
        let video = probe_video(&source, &directory, &ffprobe, None).unwrap();
        let out = directory.join("out");
        // Whether it is stopped on the way or judged at the end, the outcome is
        // the same: the original stays and nothing is left in the output folder.
        let result = convert(
            video,
            "fast",
            "mkv",
            out.clone(),
            false,
            ConversionOptions {
                min_saving_percent: 99,
                stop_if_larger: true,
                ..ConversionOptions::default()
            },
        );
        assert!(result.skipped && !result.success, "{}", result.message);
        assert!(source.exists());
        assert!(std::fs::read_dir(&out)
            .map(|mut d| d.next().is_none())
            .unwrap_or(true));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_converted_file_keeps_the_date_of_the_original() {
        let Some((ffmpeg, ffprobe)) = tools() else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let directory = scratch("mtime");
        let source = clip(&ffmpeg, &directory, "a.mp4");
        let long_ago = std::time::SystemTime::UNIX_EPOCH + Duration::from_secs(1_500_000_000);
        std::fs::OpenOptions::new()
            .write(true)
            .open(&source)
            .unwrap()
            .set_modified(long_ago)
            .unwrap();
        let video = probe_video(&source, &directory, &ffprobe, None).unwrap();
        let result = convert(
            video.clone(),
            "fast",
            "mkv",
            directory.join("out"),
            false,
            ConversionOptions::default(),
        );
        assert!(result.success, "{}", result.message);
        let modified = std::fs::metadata(&result.destination)
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(modified, long_ago, "a new file");

        // And when the original is replaced.
        let replaced = convert(
            video,
            "fast",
            "mp4",
            directory.clone(),
            true,
            ConversionOptions::default(),
        );
        assert!(replaced.replaced, "{}", replaced.message);
        let modified = std::fs::metadata(&source).unwrap().modified().unwrap();
        assert_eq!(modified, long_ago, "the replaced original");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn an_hdr_source_is_processed_in_ten_bits_even_with_the_8_bit_preset() {
        let Some((ffmpeg, ffprobe)) = tools() else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let directory = scratch("hdr");
        let source = clip(&ffmpeg, &directory, "a.mp4");
        let mut video = probe_video(&source, &directory, &ffprobe, None).unwrap();
        video.hdr = true;
        let result = convert(
            video,
            "fast",
            "mkv",
            directory.join("out"),
            false,
            ConversionOptions::default(),
        );
        assert!(result.success, "{}", result.message);
        let output = std::process::Command::new(&ffprobe)
            .args(["-v", "error", "-select_streams", "v:0"])
            .args(["-show_entries", "stream=pix_fmt", "-of", "csv=p=0"])
            .arg(&result.destination)
            .output()
            .unwrap();
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "yuv420p10le"
        );
        assert!(result.message.contains("10"), "{}", result.message);
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_time_left_follows_the_speed() {
        // 10 s of video in 5 s of work is 2x: 60 s of video left is 30 s.
        assert_eq!(estimate_seconds_left(10.0, 5.0, 60.0), Some(30));
        assert_eq!(estimate_seconds_left(0.0, 5.0, 60.0), None);
        assert_eq!(
            estimate_seconds_left(10.0, 1.0, 60.0),
            None,
            "too early to tell"
        );
    }

    #[test]
    fn a_scan_finds_earlier_conversions_and_reuses_its_cache() {
        let Some((ffmpeg, _)) = tools() else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let directory = scratch("scan");
        let library = directory.join("library");
        std::fs::create_dir_all(library.join("Fotogramma-H265")).unwrap();
        clip(&ffmpeg, &library, "done.mp4");
        clip(&ffmpeg, &library, "todo.mp4");
        // `done.mp4` already has a result next to it.
        std::fs::write(
            library.join("Fotogramma-H265").join("done.h265.mkv"),
            b"converted",
        )
        .unwrap();

        let scan = |cache: &std::path::Path| {
            let receiver = spawn_scan(
                library.clone(),
                ScanOptions {
                    cache_file: Some(cache.to_path_buf()),
                    output_dir: String::new(),
                },
                Arc::new(AtomicBool::new(false)),
                egui::Context::default(),
            );
            while let Ok(message) = receiver.recv_timeout(Duration::from_secs(60)) {
                if let ScanMessage::Completed {
                    videos,
                    from_cache,
                    already_converted,
                    ..
                } = message
                {
                    return (videos, from_cache, already_converted);
                }
            }
            panic!("the scan did not finish");
        };
        let cache = directory.join("scan-cache.json");
        let (videos, from_cache, done) = scan(&cache);
        // The result in `Fotogramma-H265` is a video of the library too (here an
        // unreadable one, since it is not a real video).
        assert_eq!(videos.len(), 3);
        assert_eq!(from_cache, 0);
        assert_eq!(done.len(), 1);
        assert!(done[0].0.ends_with("done.mp4") && done[0].1.ends_with("done.h265.mkv"));
        assert!(cache.exists(), "the cache was written");

        let (_, from_cache, done) = scan(&cache);
        assert_eq!(
            from_cache, 2,
            "the two readable files come from the cache, the unreadable one is probed again"
        );
        assert_eq!(done.len(), 1, "the earlier conversion is still recognised");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_running_queue_is_saved_and_removed_when_it_ends() {
        let Some((ffmpeg, ffprobe)) = tools() else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let directory = scratch("queue");
        let first = probe_video(
            &clip(&ffmpeg, &directory, "a.mp4"),
            &directory,
            &ffprobe,
            None,
        )
        .unwrap();
        let second = probe_video(
            &clip(&ffmpeg, &directory, "b.mp4"),
            &directory,
            &ffprobe,
            None,
        )
        .unwrap();
        let queue_file = directory.join("queue.json");
        let profile = builtin_profiles()
            .into_iter()
            .find(|p| p.key == "fast")
            .unwrap();
        let receiver = spawn_conversion(
            vec![first, second],
            profile,
            directory.join("out"),
            "mkv".to_string(),
            false,
            ConversionOptions {
                queue_file: Some(queue_file.clone()),
                ..ConversionOptions::default()
            },
            Arc::new(AtomicBool::new(false)),
            egui::Context::default(),
        );
        let mut saw_the_queue = false;
        while let Ok(message) = receiver.recv_timeout(Duration::from_secs(120)) {
            match message {
                ConversionMessage::FileStarted { index: 1, .. } => {
                    let queue = PendingQueue::load(&queue_file).expect("saved when it starts");
                    assert_eq!(queue.videos.len(), 2);
                    saw_the_queue = true;
                }
                ConversionMessage::FileStarted { index: 2, .. } => {
                    let queue = PendingQueue::load(&queue_file).expect("still saved");
                    assert_eq!(
                        queue.videos.len(),
                        1,
                        "the finished file is gone from the queue"
                    );
                }
                ConversionMessage::Completed(_) => break,
                _ => {}
            }
        }
        assert!(saw_the_queue);
        assert!(!queue_file.exists(), "nothing to resume once it ended");
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn a_paused_queue_waits_and_then_goes_on() {
        let Some((ffmpeg, ffprobe)) = tools() else {
            eprintln!("skipped: no ffmpeg");
            return;
        };
        let directory = scratch("pause");
        let video = probe_video(
            &clip(&ffmpeg, &directory, "a.mp4"),
            &directory,
            &ffprobe,
            None,
        )
        .unwrap();
        let pause = Arc::new(AtomicBool::new(true));
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
            ConversionOptions {
                pause: pause.clone(),
                ..ConversionOptions::default()
            },
            Arc::new(AtomicBool::new(false)),
            egui::Context::default(),
        );
        // Paused: nothing starts.
        assert!(receiver.recv_timeout(Duration::from_millis(800)).is_err());
        pause.store(false, Ordering::Relaxed);
        let mut finished = false;
        while let Ok(message) = receiver.recv_timeout(Duration::from_secs(120)) {
            if matches!(message, ConversionMessage::Completed(_)) {
                finished = true;
                break;
            }
        }
        assert!(finished, "it goes on once resumed");
        let _ = std::fs::remove_dir_all(&directory);
    }
}
