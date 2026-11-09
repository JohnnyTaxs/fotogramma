//! Safe H.265 conversion profiles, output allocation and command generation.

use crate::i18n::tr_str;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioMode {
    Copy,
    Aac,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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

/// What a video is, as far as the encoder settings go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    /// Animation: flat areas and sharp lines, where `-tune animation` helps.
    Anime,
    /// Live action (films and series): x265's own defaults are tuned for it.
    Live,
}

impl MediaKind {
    /// The x265 tune for this kind. x265 has no "film" tune, so live action
    /// uses none.
    pub fn tune(self) -> Option<&'static str> {
        match self {
            MediaKind::Anime => Some("animation"),
            MediaKind::Live => None,
        }
    }

    /// `anime`, `live`, anything else (`auto`) → `None`.
    pub fn from_setting(value: &str) -> Option<MediaKind> {
        match value {
            "anime" => Some(MediaKind::Anime),
            "live" => Some(MediaKind::Live),
            _ => None,
        }
    }
}

const ANIME_WORDS: [&str; 6] = [
    "anime",
    "animes",
    "animazione",
    "animation",
    "cartoni",
    "cartoon",
];
const LIVE_WORDS: [&str; 13] = [
    "film",
    "films",
    "movie",
    "movies",
    "serie",
    "series",
    "tv",
    "telefilm",
    "show",
    "shows",
    "documentari",
    "documentary",
    "documentaries",
];

/// Guess the kind from the folders above a file. Any folder that names anime
/// makes it anime (so `Anime/Frieren The Movie/` stays anime even though the
/// title says "movie"); otherwise a folder that names films or series makes it
/// live action. The file name is ignored: both worlds use `S01E02`.
pub fn detect_kind(path: &Path) -> Option<MediaKind> {
    let mut live = false;
    for folder in path.parent()?.ancestors() {
        let Some(name) = folder.file_name() else {
            continue;
        };
        let name = name.to_string_lossy().to_lowercase();
        for token in name
            .split(|c: char| !c.is_alphanumeric())
            .filter(|token| !token.is_empty())
        {
            if ANIME_WORDS.contains(&token) {
                return Some(MediaKind::Anime);
            }
            live |= LIVE_WORDS.contains(&token);
        }
    }
    live.then_some(MediaKind::Live)
}

/// The kind of a whole selection: the kinds found must agree. Videos with no
/// hint are ignored; a mixed selection gives `None`.
pub fn detect_selection<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Option<MediaKind> {
    let mut found = None;
    for path in paths {
        match (found, detect_kind(path)) {
            (_, None) => {}
            (None, Some(kind)) => found = Some(kind),
            (Some(a), Some(b)) if a == b => {}
            _ => return None,
        }
    }
    found
}

/// MP4 holds only `mov_text` subtitles. Text subtitles from an MKV (SRT, ASS)
/// and graphic ones (PGS, DVD) make `ffmpeg` write nothing, so a file that
/// has any of them goes into MKV even when MP4 was asked for.
pub fn container_for<'a>(requested: &'a str, subtitle_codecs: &[String]) -> &'a str {
    if requested == "mp4" && subtitle_codecs.iter().any(|codec| codec != "mov_text") {
        "mkv"
    } else {
        requested
    }
}

/// HDR needs more than 8 bits, so an 8-bit preset is raised to 10 bits for an
/// HDR source: the HDR tags and mastering data are kept by FFmpeg, but not
/// meaningfully in 8 bits.
pub fn profile_for_video(profile: &ConversionProfile, hdr: bool) -> ConversionProfile {
    let mut profile = profile.clone();
    if hdr && profile.pixel_format == "yuv420p" {
        profile.pixel_format = "yuv420p10le".to_string();
    }
    profile
}

pub fn builtin_profiles() -> Vec<ConversionProfile> {
    let mut maximum = ConversionProfile::new(
        "max_quality",
        tr_str("Massima qualità e compressione", "Maximum quality and compression"),
        tr_str(
            "Il file più piccolo possibile a questa qualità (CRF 20, preset veryslow, AQ mode 3, 10 bit). È molto lento: indicativamente il triplo di Alta qualità per risparmiare solo il 2-3% di spazio. Un film lungo può richiedere ore.",
            "The smallest possible file at this quality (CRF 20, veryslow preset, AQ mode 3, 10-bit). It is very slow: roughly three times High quality to save only 2-3% of space. A long film can take hours.",
        ),
        20,
        "veryslow",
        "yuv420p10le",
    );
    maximum.aq_mode = Some(3);

    let mut high = ConversionProfile::new(
        "high_quality",
        tr_str("Alta qualità · consigliato per l'archivio", "High quality · recommended for the archive"),
        tr_str(
            "Qualità quasi identica alla massima in molto meno tempo (CRF 20, preset slow, AQ mode 3, 10 bit). È il compromesso giusto per un archivio.",
            "Quality almost identical to Maximum in much less time (CRF 20, slow preset, AQ mode 3, 10-bit). The right compromise for an archive.",
        ),
        20,
        "slow",
        "yuv420p10le",
    );
    high.aq_mode = Some(3);

    vec![
        maximum,
        high,
        ConversionProfile::new(
            "balanced",
            tr_str("Intermedio · buona qualità", "Intermediate · good quality"),
            tr_str(
                "Buon equilibrio tra qualità, spazio e tempo (CRF 22, preset medium, 10 bit): file più piccolo di Alta qualità, con un po' meno qualità.",
                "A good balance of quality, size and time (CRF 22, medium preset, 10-bit): a smaller file than High quality, with a little less quality.",
            ),
            22,
            "medium",
            "yuv420p10le",
        ),
        ConversionProfile::new(
            "fast",
            tr_str("Veloce · transcode rapido", "Fast · quick transcode"),
            tr_str(
                "Massima velocità (CRF 26, preset ultrafast, 8 bit): molto più rapido, ma la qualità è più bassa e il file si comprime poco.",
                "Maximum speed (CRF 26, ultrafast preset, 8-bit): much faster, but quality is lower and the file is compressed less.",
            ),
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
        label: tr_str("Personalizzato", "Custom").to_string(),
        description: tr_str(
            "Impostazioni scelte manualmente.",
            "Settings chosen manually.",
        )
        .to_string(),
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
        // `hev1` is what FFmpeg writes by default; QuickTime and Safari only
        // play HEVC in MP4 when it is tagged `hvc1`.
        args.extend([
            "-tag:v".to_string(),
            "hvc1".to_string(),
            "-movflags".to_string(),
            "+faststart".to_string(),
        ]);
    }
    args.extend(["-progress".to_string(), "pipe:1".to_string()]);
    args.push(destination.to_string_lossy().into_owned());
    args
}

/// Where a conversion of `source` puts its result: the folder chosen in the
/// settings, or a `Fotogramma-H265` folder next to the original.
pub fn output_dir_for(source: &Path, configured: &str) -> PathBuf {
    let configured = configured.trim();
    if configured.is_empty() {
        source
            .parent()
            .map(|parent| parent.join("Fotogramma-H265"))
            .unwrap_or_else(|| PathBuf::from("Fotogramma-H265"))
    } else {
        PathBuf::from(configured)
    }
}

/// The file an earlier conversion made from `source`, if it is still in
/// `output_dir`: `name.h265.mkv` or `name.h265.mp4`, not empty.
pub fn existing_output(source: &Path, output_dir: &Path) -> Option<PathBuf> {
    let stem = source.file_stem()?.to_string_lossy().into_owned();
    ["mkv", "mp4"]
        .iter()
        .map(|extension| output_dir.join(format!("{stem}.h265.{extension}")))
        .find(|candidate| {
            std::fs::metadata(candidate)
                .map(|metadata| metadata.is_file() && metadata.len() > 0)
                .unwrap_or(false)
        })
}

/// Never overwrite: choose `name.h265.<container>` and add a counter on collisions.
pub fn allocate_output_path(
    source: &Path,
    destination_dir: &Path,
    container: &str,
) -> Result<PathBuf, String> {
    if container != "mkv" && container != "mp4" {
        return Err(tr_str("Contenitore non supportato", "Unsupported container").to_string());
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
        return Err(tr_str(
            "La sostituzione è disponibile solo per file MKV e MP4",
            "Replacement is only available for MKV and MP4 files",
        )
        .to_string());
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
        let high = profile("high_quality").unwrap();
        assert_eq!((high.crf, high.preset.as_str()), (20, "slow"));
        assert_eq!(high.aq_mode, Some(3));
        assert_eq!(high.pixel_format, "yuv420p10le");
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
        assert!(
            args.windows(2).any(|pair| pair == ["-tag:v", "hvc1"]),
            "MP4 output is tagged hvc1 so Apple players open it"
        );
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

    #[test]
    fn custom_profile_arguments_carry_every_choice() {
        let profile = custom_profile(
            22,
            "slow",
            "yuv420p10le",
            Some("grain"),
            Some(3),
            AudioMode::Aac,
            160,
        );
        let args = build_ffmpeg_args(&profile, Path::new("/v/a.mkv"), Path::new("/v/a.h265.mp4"));
        let has = |pair: [&str; 2]| args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1]);
        assert!(has(["-crf", "22"]));
        assert!(has(["-preset", "slow"]));
        assert!(has(["-tune", "grain"]));
        assert!(has(["-x265-params", "aq-mode=3"]));
        assert!(has(["-c:a", "aac"]));
        assert!(has(["-b:a", "160k"]));
        assert!(has(["-movflags", "+faststart"]));
    }

    #[test]
    fn every_profile_maps_all_streams_and_never_overwrites() {
        for profile in builtin_profiles() {
            let args =
                build_ffmpeg_args(&profile, Path::new("/v/a.mkv"), Path::new("/v/a.h265.mkv"));
            assert_eq!(
                args.first().map(String::as_str),
                Some("-n"),
                "{}",
                profile.key
            );
            assert!(
                args.windows(2).any(|w| w[0] == "-map" && w[1] == "0"),
                "{}",
                profile.key
            );
        }
    }

    #[test]
    fn an_empty_tune_is_left_out() {
        let profile = custom_profile(
            23,
            "medium",
            "yuv420p",
            Some(""),
            None,
            AudioMode::Copy,
            128,
        );
        let args = build_ffmpeg_args(&profile, Path::new("/v/a.mkv"), Path::new("/v/a.h265.mkv"));
        assert!(!args.iter().any(|value| value == "-tune"));
        assert!(!args.iter().any(|value| value == "-x265-params"));
    }

    #[test]
    fn x265_has_no_film_tune() {
        assert_eq!(MediaKind::Anime.tune(), Some("animation"));
        assert_eq!(MediaKind::Live.tune(), None);
    }

    #[test]
    fn the_kind_comes_from_the_folders() {
        let kind = |path: &str| detect_kind(Path::new(path));
        assert_eq!(
            kind("/Media/Anime/Frieren/S01E01.mkv"),
            Some(MediaKind::Anime)
        );
        assert_eq!(
            kind("/Media/Serie TV/Dark/S01E01.mkv"),
            Some(MediaKind::Live)
        );
        assert_eq!(
            kind("/Media/serie-tv/Dark/S01E01.mkv"),
            Some(MediaKind::Live)
        );
        assert_eq!(
            kind("/Media/Movies/Heat (1995)/Heat.mkv"),
            Some(MediaKind::Live)
        );
        assert_eq!(kind("/Media/Film/Alien.mkv"), Some(MediaKind::Live));
        assert_eq!(kind("/Media/Animazione/Coco.mkv"), Some(MediaKind::Anime));
        assert_eq!(kind("/Media/Misc/clip.mkv"), None);
        assert_eq!(kind("clip.mkv"), None);
    }

    #[test]
    fn anime_anywhere_in_the_path_wins_and_the_file_name_is_ignored() {
        let kind = |path: &str| detect_kind(Path::new(path));
        assert_eq!(
            kind("/Film/Anime/Spirited Away.mkv"),
            Some(MediaKind::Anime)
        );
        assert_eq!(kind("/Anime/Film/Your Name.mkv"), Some(MediaKind::Anime));
        assert_eq!(
            kind("/Anime/Frieren The Movie/a.mkv"),
            Some(MediaKind::Anime)
        );
        assert_eq!(kind("/Media/Anime Movies/x.mkv"), Some(MediaKind::Anime));
        assert_eq!(kind("/Media/Misc/Anime Movie.mkv"), None);
        assert_eq!(kind("/Filmati/clip.mkv"), None, "a word must match whole");
    }

    #[test]
    fn a_selection_needs_agreeing_hints() {
        let anime = Path::new("/Anime/a.mkv");
        let live = Path::new("/Film/b.mkv");
        let none = Path::new("/Misc/c.mkv");
        assert_eq!(detect_selection([anime, none]), Some(MediaKind::Anime));
        assert_eq!(detect_selection([anime, anime]), Some(MediaKind::Anime));
        assert_eq!(detect_selection([anime, live]), None);
        assert_eq!(detect_selection([none]), None);
    }

    #[test]
    fn settings_map_to_kinds() {
        assert_eq!(MediaKind::from_setting("anime"), Some(MediaKind::Anime));
        assert_eq!(MediaKind::from_setting("live"), Some(MediaKind::Live));
        assert_eq!(MediaKind::from_setting("auto"), None);
    }

    #[test]
    fn mp4_is_kept_only_when_it_can_hold_the_subtitles() {
        let codecs = |list: &[&str]| list.iter().map(|c| c.to_string()).collect::<Vec<_>>();
        assert_eq!(container_for("mp4", &[]), "mp4");
        assert_eq!(container_for("mp4", &codecs(&["mov_text"])), "mp4");
        assert_eq!(container_for("mp4", &codecs(&["subrip"])), "mkv");
        assert_eq!(
            container_for("mp4", &codecs(&["mov_text", "hdmv_pgs_subtitle"])),
            "mkv"
        );
        assert_eq!(container_for("mkv", &codecs(&["subrip"])), "mkv");
    }

    #[test]
    fn hdr_sources_never_get_an_8_bit_pipeline() {
        let fast = profile("fast").unwrap();
        assert_eq!(fast.pixel_format, "yuv420p");
        assert_eq!(profile_for_video(&fast, true).pixel_format, "yuv420p10le");
        assert_eq!(profile_for_video(&fast, false).pixel_format, "yuv420p");
        let balanced = profile("balanced").unwrap();
        assert_eq!(profile_for_video(&balanced, true), balanced);
    }

    #[test]
    fn the_output_folder_is_the_configured_one_or_next_to_the_original() {
        let source = Path::new("/Media/Show/a.mkv");
        assert_eq!(
            output_dir_for(source, ""),
            PathBuf::from("/Media/Show").join("Fotogramma-H265")
        );
        assert_eq!(
            output_dir_for(source, "  /Volumes/Out "),
            PathBuf::from("/Volumes/Out")
        );
    }

    #[test]
    fn an_earlier_result_is_found_in_the_output_folder() {
        let directory = std::env::temp_dir().join(format!("foto-existing-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        let source = Path::new("/lib/Show/Episode 1.mkv");
        assert_eq!(existing_output(source, &directory), None);
        std::fs::write(directory.join("Episode 1.h265.mp4"), b"").unwrap();
        assert_eq!(
            existing_output(source, &directory),
            None,
            "an empty file is not a result"
        );
        std::fs::write(directory.join("Episode 1.h265.mp4"), b"video").unwrap();
        assert_eq!(
            existing_output(source, &directory),
            Some(directory.join("Episode 1.h265.mp4"))
        );
        assert_eq!(
            existing_output(Path::new("/lib/Other.mkv"), &directory),
            None
        );
        let _ = std::fs::remove_dir_all(&directory);
    }
}
