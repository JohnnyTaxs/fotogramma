//! Full validation and atomic replacement for verified H.265 outputs.

use crate::media_tools::{ffprobe_path, prepare_command};
use crate::scanner::{probe_video, stream_categories, VideoInfo};
use std::fs;
use std::path::Path;
use std::process::Stdio;

pub fn stream_counts(
    path: &Path,
    ffprobe: &Path,
) -> Result<std::collections::BTreeMap<String, usize>, String> {
    let mut command = prepare_command(ffprobe);
    command
        .arg("-v")
        .arg("error")
        .arg("-show_entries")
        .arg("stream=codec_type")
        .arg("-of")
        .arg("json")
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command
        .output()
        .map_err(|error| format!("Impossibile leggere gli stream: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Impossibile leggere gli stream".to_string()
        } else {
            stderr
        });
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stream_categories(&stdout)
}

/// Validate metadata, duration, streams and a full decode before replacement.
pub fn validate_conversion_output(
    original: &VideoInfo,
    output: &Path,
    ffprobe: &Path,
    ffmpeg: &Path,
) -> (bool, String) {
    let parent = output.parent().unwrap_or_else(|| Path::new("."));
    let metadata = match probe_video(output, parent, ffprobe, None) {
        Ok(metadata) => metadata,
        Err(_) => return (false, "Verifica interrotta".to_string()),
    };
    if let Some(error) = metadata.error {
        return (false, error);
    }
    if metadata.codec.as_deref() != Some("hevc") {
        return (false, "Il codec di output non è HEVC".to_string());
    }

    match (original.duration, metadata.duration) {
        (Some(original_duration), Some(output_duration)) => {
            let tolerance = 1.0_f64.max(original_duration * 0.01);
            if (original_duration - output_duration).abs() > tolerance {
                return (
                    false,
                    format!(
                        "Durata non coerente: originale {original_duration:.2}s, output {output_duration:.2}s"
                    ),
                );
            }
        }
        (Some(_), None) => {
            return (
                false,
                "Durata del file convertito non disponibile".to_string(),
            );
        }
        _ => {}
    }

    match (
        stream_counts(&original.path, ffprobe),
        stream_counts(output, ffprobe),
    ) {
        (Ok(original_streams), Ok(output_streams)) => {
            if original_streams != output_streams {
                return (
                    false,
                    "Il numero o il tipo di stream non coincide con l’originale".to_string(),
                );
            }
        }
        (Err(error), _) | (_, Err(error)) => return (false, error),
    }

    let mut command = prepare_command(ffmpeg);
    command
        .arg("-v")
        .arg("error")
        .arg("-nostats")
        .arg("-i")
        .arg(output)
        .arg("-f")
        .arg("null")
        .arg("-")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let result = match command.output() {
        Ok(result) => result,
        Err(error) => {
            return (false, format!("Decodifica completa non riuscita: {error}"));
        }
    };
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        let stdout = String::from_utf8_lossy(&result.stdout);
        let text = format!("{stderr}{stdout}");
        let last = text.lines().last().map(|line| line.trim()).unwrap_or("");
        let details = if last.is_empty() {
            "errore sconosciuto"
        } else {
            last
        };
        return (
            false,
            format!("Decodifica completa non riuscita: {details}"),
        );
    }
    (true, "Verifica completa superata".to_string())
}

/// Atomically replace the original, restoring a backup on any failure.
pub fn replace_verified_output(
    original_path: &Path,
    output: &Path,
    ffprobe: &Path,
) -> (bool, String) {
    let source = original_path;
    let Some(file_name) = source.file_name() else {
        return (false, "Percorso originale non valido".to_string());
    };
    let backup =
        source.with_file_name(format!("{}.fotogramma-backup", file_name.to_string_lossy()));
    if backup.exists() {
        return (
            false,
            format!(
                "Backup già presente: {}. Spostalo o eliminalo prima di riprovare.",
                backup.display()
            ),
        );
    }
    let result = (|| -> Result<(), String> {
        fs::rename(source, &backup).map_err(|error| error.to_string())?;
        if let Err(error) = fs::rename(output, source) {
            let restore = fs::rename(&backup, source);
            return Err(match restore {
                Ok(()) => format!("Impossibile sostituire l'originale: {error}"),
                Err(restore_error) => format!(
                    "Impossibile sostituire l'originale: {error}. Ripristino non riuscito: {restore_error}. Backup: {}",
                    backup.display()
                ),
            });
        }
        let parent = source.parent().unwrap_or_else(|| Path::new("."));
        let final_info = probe_video(source, parent, ffprobe, None)
            .map_err(|_| "Controllo finale interrotto".to_string())?;
        if final_info.category != crate::scanner::Category::Compatible {
            return Err(final_info
                .error
                .unwrap_or_else(|| "Controllo finale del codec non riuscito".to_string()));
        }
        fs::remove_file(&backup).map_err(|error| error.to_string())?;
        Ok(())
    })();

    match result {
        Ok(()) => (
            true,
            "Originale sostituito dopo verifica completa".to_string(),
        ),
        Err(error) => {
            if backup.exists() {
                match restore_backup(&backup, source) {
                    Ok(()) => (false, format!("Originale non modificato: {error}")),
                    Err(restore_error) => (
                        false,
                        format!(
                            "{error}. Ripristino non riuscito: {restore_error}. Backup: {}",
                            backup.display()
                        ),
                    ),
                }
            } else {
                (false, format!("Originale non modificato: {error}"))
            }
        }
    }
}

/// Put the backup back in place. On Windows a rename cannot replace an
/// existing file, so the failed output is removed first.
fn restore_backup(backup: &Path, source: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        if source.exists() {
            let _ = fs::remove_file(source);
        }
    }
    fs::rename(backup, source)
}

/// Resolve ffprobe with the same rules used by the scan.
pub fn resolve_ffprobe() -> Result<std::path::PathBuf, String> {
    ffprobe_path()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::VideoInfo;

    fn video(path: &Path) -> VideoInfo {
        VideoInfo {
            path: path.to_path_buf(),
            relative_path: "one.mkv".into(),
            name: "one.mkv".into(),
            folder: String::new(),
            extension: "mkv".into(),
            category: crate::scanner::Category::Convert,
            codec: Some("h264".into()),
            size: None,
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            error: None,
        }
    }

    #[test]
    fn backup_is_never_deleted_before_validation() {
        let directory = std::env::temp_dir().join(format!("foto-replace-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let original = directory.join("one.mkv");
        let output = directory.join("one.h265.partial.mkv");
        fs::write(&original, b"original").unwrap();
        fs::write(&output, b"converted").unwrap();
        let backup = directory.join("one.mkv.fotogramma-backup");
        fs::write(&backup, b"stale").unwrap();

        let (replaced, message) =
            replace_verified_output(&original, &output, Path::new("/missing/ffprobe"));
        assert!(!replaced);
        assert!(message.contains("Backup già presente"));
        assert_eq!(fs::read(&original).unwrap(), b"original");
        assert_eq!(fs::read(&output).unwrap(), b"converted");
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn failed_final_check_restores_the_original() {
        let directory = std::env::temp_dir().join(format!("foto-restore-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let original = directory.join("one.mkv");
        let output = directory.join("one.h265.partial.mkv");
        fs::write(&original, b"original").unwrap();
        fs::write(&output, b"converted").unwrap();

        let (replaced, _) =
            replace_verified_output(&original, &output, Path::new("/missing/ffprobe"));
        assert!(!replaced);
        assert_eq!(fs::read(&original).unwrap(), b"original");
        assert!(!directory.join("one.mkv.fotogramma-backup").exists());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn duration_tolerance_is_one_percent_or_one_second() {
        let base = video(Path::new("/videos/one.mkv"));
        assert!(base.duration.unwrap() * 0.01 < 1.0);
    }

    #[test]
    fn replacing_a_path_without_a_file_name_is_refused() {
        let (replaced, message) =
            replace_verified_output(Path::new("/"), Path::new("/tmp/x"), Path::new("/missing"));
        assert!(!replaced);
        assert!(message.contains("Percorso originale non valido"));
    }

    #[test]
    fn validation_fails_when_the_output_cannot_be_probed() {
        let directory = std::env::temp_dir().join(format!("foto-validate-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        let output = directory.join("one.h265.partial.mkv");
        fs::write(&output, b"not a video").unwrap();
        let (valid, _) = validate_conversion_output(
            &video(&directory.join("one.mkv")),
            &output,
            Path::new("/missing/ffprobe"),
            Path::new("/missing/ffmpeg"),
        );
        assert!(!valid);
        let _ = fs::remove_dir_all(&directory);
    }
}
