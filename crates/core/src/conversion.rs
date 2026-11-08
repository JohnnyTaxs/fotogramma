//! Safe H.265 conversion profiles, output allocation and command generation.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioMode {
    Copy,
    Aac,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ConversionProfile {
    pub key: String,
    pub label: String,
    pub description: String,
    pub crf: u8,
    pub preset: String,
    pub pixel_format: String,
    pub tune: Option<String>,
    pub aq_mode: Option<u8>,
    pub audio_mode: AudioMode,
    pub audio_bitrate: u32,
}

impl ConversionProfile {
    fn new(
        key: &str,
        label: &str,
        description: &str,
        crf: u8,
        preset: &str,
        pixel_format: &str,
    ) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            description: description.to_string(),
            crf,
            preset: preset.to_string(),
            pixel_format: pixel_format.to_string(),
            tune: None,
            aq_mode: None,
            audio_mode: AudioMode::Copy,
            audio_bitrate: 160,
        }
    }
}

pub const PRESETS: [&str; 9] = [
    "ultrafast",
    "superfast",
    "veryfast",
    "faster",
    "fast",
    "medium",
    "slow",
    "slower",
    "veryslow",
];

pub fn builtin_profiles() -> Vec<ConversionProfile> {
    let mut maximum = ConversionProfile::new(
        "max_quality",
        "Massima qualità e compressione",
        "Il miglior rapporto qualità/dimensione, senza fretta: CRF 20, preset veryslow e AQ mode 3.",
        20,
        "veryslow",
        "yuv420p10le",
    );
    maximum.aq_mode = Some(3);

    vec![
        maximum,
        ConversionProfile::new(
            "balanced",
            "Intermedio · buona qualità",
            "Buon equilibrio tra qualità, spazio e tempo: CRF 22 e preset medium.",
            22,
            "medium",
            "yuv420p10le",
        ),
        ConversionProfile::new(
            "fast",
            "Veloce · transcode rapido",
            "Massima velocità disponibile: CRF 26, preset ultrafast e video 8-bit.",
            26,
            "ultrafast",
            "yuv420p",
        ),
    ]
}

pub fn profile(key: &str) -> Option<ConversionProfile> {
    builtin_profiles().into_iter().find(|item| item.key == key)
}

#[allow(clippy::too_many_arguments)]
pub fn custom_profile(
    crf: u8,
    preset: &str,
    pixel_format: &str,
    tune: Option<&str>,
    aq_mode: Option<u8>,
    audio_mode: AudioMode,
    audio_bitrate: u32,
) -> ConversionProfile {
    ConversionProfile {
        key: "custom".to_string(),
        label: "Personalizzato".to_string(),
        description: "Impostazioni scelte manualmente.".to_string(),
        crf,
        preset: preset.to_string(),
        pixel_format: pixel_format.to_string(),
        tune: tune
            .map(|value| value.to_string())
            .filter(|value| !value.is_empty()),
        aq_mode,
        audio_mode,
        audio_bitrate,
    }
}

pub fn build_ffmpeg_args(
    profile: &ConversionProfile,
    source: &Path,
    destination: &Path,
) -> Vec<String> {
    let mut args: Vec<String> = ["-n", "-hide_banner", "-loglevel", "error", "-nostats", "-i"]
        .iter()
        .map(|value| value.to_string())
        .collect();
    args.push(source.to_string_lossy().into_owned());
    args.extend(
        ["-map", "0", "-c:v", "libx265", "-preset"]
            .iter()
            .map(|value| value.to_string()),
    );
    args.push(profile.preset.clone());
    args.extend(["-crf".to_string(), profile.crf.to_string()]);
    args.extend(["-pix_fmt".to_string(), profile.pixel_format.clone()]);
    if let Some(tune) = profile.tune.as_deref().filter(|value| !value.is_empty()) {
        args.extend(["-tune".to_string(), tune.to_string()]);
    }
    if let Some(aq_mode) = profile.aq_mode {
        args.extend(["-x265-params".to_string(), format!("aq-mode={aq_mode}")]);
    }
    match profile.audio_mode {
        AudioMode::Copy => args.extend(["-c:a".to_string(), "copy".to_string()]),
        AudioMode::Aac => args.extend([
            "-c:a".to_string(),
            "aac".to_string(),
            "-b:a".to_string(),
            format!("{}k", profile.audio_bitrate),
        ]),
    }
    args.extend(
        [
            "-c:s",
            "copy",
            "-c:d",
            "copy",
            "-map_metadata",
            "0",
            "-map_chapters",
            "0",
        ]
        .iter()
        .map(|value| value.to_string()),
    );
    if destination
        .to_string_lossy()
        .to_lowercase()
        .ends_with(".mp4")
    {
        args.extend(["-movflags".to_string(), "+faststart".to_string()]);
    }
    args.extend(["-progress".to_string(), "pipe:1".to_string()]);
    args.push(destination.to_string_lossy().into_owned());
    args
}

/// Never overwrite: choose `name.h265.<container>` and add a counter on collisions.
pub fn allocate_output_path(
    source: &Path,
    destination_dir: &Path,
    container: &str,
) -> Result<PathBuf, String> {
    if container != "mkv" && container != "mp4" {
        return Err("Contenitore non supportato".to_string());
    }
    let stem = source
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".to_string());
    let suffix = format!(".h265.{container}");
    let mut candidate = destination_dir.join(format!("{stem}{suffix}"));
    let mut counter = 1;
    while candidate.exists() {
        candidate = destination_dir.join(format!("{stem} ({counter}){suffix}"));
        counter += 1;
    }
    Ok(candidate)
}

/// Allocate a same-filesystem temporary path used only after full validation.
pub fn allocate_partial_path(source: &Path) -> Result<PathBuf, String> {
    let extension = source
        .extension()
        .map(|value| value.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if extension != "mkv" && extension != "mp4" {
        return Err("La sostituzione è disponibile solo per file MKV e MP4".to_string());
    }
    let stem = source
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| "video".to_string());
    let original_suffix = source
        .extension()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let parent = source.parent().unwrap_or_else(|| Path::new("."));
    let mut candidate = parent.join(format!("{stem}.h265.partial.{original_suffix}"));
    let mut counter = 1;
    while candidate.exists() {
        candidate = parent.join(format!("{stem}.h265.partial-{counter}.{original_suffix}"));
        counter += 1;
    }
    Ok(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn balanced_profile_matches_the_python_defaults() {
        let balanced = profile("balanced").expect("balanced profile");
        assert_eq!(balanced.crf, 22);
        assert_eq!(balanced.preset, "medium");
        assert_eq!(balanced.pixel_format, "yuv420p10le");
        assert_eq!(balanced.audio_mode, AudioMode::Copy);
    }

    #[test]
    fn maximum_quality_uses_aq_mode_three() {
        let maximum = profile("max_quality").unwrap();
        assert_eq!(maximum.aq_mode, Some(3));
        assert_eq!(maximum.preset, "veryslow");
    }

    #[test]
    fn ffmpeg_arguments_never_overwrite_and_keep_every_stream() {
        let balanced = profile("balanced").unwrap();
        let args = build_ffmpeg_args(
            &balanced,
            Path::new("/videos/one.mp4"),
            Path::new("/videos/one.h265.mkv"),
        );
        assert!(args.contains(&"-n".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["-map", "0"]));
        assert!(args.windows(2).any(|pair| pair == ["-c:v", "libx265"]));
        assert!(args.windows(2).any(|pair| pair == ["-c:a", "copy"]));
        assert!(args.windows(2).any(|pair| pair == ["-progress", "pipe:1"]));
        assert!(!args.contains(&"-y".to_string()));
        assert_eq!(args.last().unwrap(), "/videos/one.h265.mkv");
    }

    #[test]
    fn maximum_quality_adds_x265_parameters() {
        let maximum = profile("max_quality").unwrap();
        let args = build_ffmpeg_args(
            &maximum,
            Path::new("/videos/one.mkv"),
            Path::new("/videos/one.h265.mkv"),
        );
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-x265-params", "aq-mode=3"]));
    }

    #[test]
    fn mp4_destination_adds_faststart() {
        let fast = profile("fast").unwrap();
        let args = build_ffmpeg_args(
            &fast,
            Path::new("/videos/one.mov"),
            Path::new("/videos/one.h265.MP4"),
        );
        assert!(args
            .windows(2)
            .any(|pair| pair == ["-movflags", "+faststart"]));
    }

    #[test]
    fn output_allocation_adds_a_counter() {
        let directory = std::env::temp_dir().join(format!("foto-alloc-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = directory.join("Tramonto.mkv");
        std::fs::write(&source, b"x").unwrap();
        let first = allocate_output_path(&source, &directory, "mkv").unwrap();
        assert!(first.ends_with("Tramonto.h265.mkv"));
        std::fs::write(&first, b"x").unwrap();
        let second = allocate_output_path(&source, &directory, "mkv").unwrap();
        assert!(second.ends_with("Tramonto (1).h265.mkv"));
        let _ = std::fs::remove_dir_all(&directory);
    }

    #[test]
    fn partial_path_is_same_directory_and_rejects_webm() {
        let source = Path::new("/videos/Tramonto.mkv");
        let partial = allocate_partial_path(source).unwrap();
        assert_eq!(partial.parent(), source.parent());
        assert!(partial
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("Tramonto.h265.partial"));
        assert!(allocate_partial_path(Path::new("/videos/one.webm")).is_err());
    }
}
