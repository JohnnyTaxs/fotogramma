//! Small persisted preferences: directory, window size and the user's
//! defaults (accepted codecs, conversion defaults). Scan results are never
//! stored.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Codecs the user can mark as already acceptable: ffprobe name and label.
pub const KNOWN_CODECS: [(&str, &str); 4] = [
    ("hevc", "H.265 · HEVC"),
    ("av1", "AV1 · AOMedia"),
    ("h264", "H.264 · AVC"),
    ("vp9", "VP9 · WebM"),
];

/// A named set of encoder settings the user saved from the custom profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedPreset {
    pub name: String,
    pub crf: u8,
    pub preset: String,
    pub pixel: String,
    pub tune: String,
    pub aq: Option<u8>,
    pub audio_index: usize,
    pub container: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub directory: String,
    #[serde(default)]
    pub window_width: Option<f32>,
    #[serde(default)]
    pub window_height: Option<f32>,
    /// The user's own presets.
    #[serde(default)]
    pub presets: Vec<SavedPreset>,
    /// Reuse the results of an earlier scan for files that did not change.
    #[serde(default = "default_true")]
    pub scan_cache: bool,
    /// Keep the computer awake while a conversion runs.
    #[serde(default = "default_true")]
    pub prevent_sleep: bool,
    /// Keep the original, and drop the result, when it saves less than this
    /// percentage. 0 keeps every result.
    #[serde(default)]
    pub min_saving_percent: u8,
    /// Stop a file as soon as it is clearly heading for a result larger than
    /// the original (or than the minimum saving allows).
    #[serde(default = "default_true")]
    pub stop_if_larger: bool,
    /// Interface language: `auto` (follow the system), `it` or `en`.
    #[serde(default = "default_language")]
    pub language: String,
    /// Ask GitHub for a newer release at startup. Off until the user agrees
    /// to it in the first-run question or in Settings.
    #[serde(default)]
    pub check_updates: bool,
    /// The first-run question about updates was answered.
    #[serde(default)]
    pub update_choice_made: bool,
    /// Release tag the user chose to skip.
    #[serde(default)]
    pub skipped_update: String,
    /// Codecs treated as already compatible.
    #[serde(default = "default_accepted_codecs")]
    pub accepted_codecs: Vec<String>,
    /// Pre-select "replace the original" in the conversion dialog.
    #[serde(default)]
    pub replace_original: bool,
    /// Conversion profile key pre-selected in the conversion dialog.
    #[serde(default = "default_profile")]
    pub default_profile: String,
    /// Content type for the encoder: `auto` (read the path), `anime` or `live`.
    #[serde(default = "default_content_type")]
    pub content_type: String,
    /// Folder for converted files; empty means a `Fotogramma-H265` folder
    /// next to the originals.
    #[serde(default)]
    pub output_dir: String,
    /// Output container for new files: `mkv` or `mp4`.
    #[serde(default = "default_container")]
    pub default_container: String,
}

fn default_true() -> bool {
    true
}

fn default_language() -> String {
    "auto".to_string()
}

fn default_accepted_codecs() -> Vec<String> {
    vec!["hevc".to_string(), "av1".to_string()]
}

fn default_profile() -> String {
    "balanced".to_string()
}

fn default_content_type() -> String {
    "auto".to_string()
}

fn default_container() -> String {
    "mkv".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            directory: String::new(),
            window_width: None,
            window_height: None,
            language: default_language(),
            check_updates: false,
            update_choice_made: false,
            presets: Vec::new(),
            scan_cache: true,
            prevent_sleep: true,
            min_saving_percent: 0,
            stop_if_larger: true,
            skipped_update: String::new(),
            accepted_codecs: default_accepted_codecs(),
            replace_original: false,
            default_profile: default_profile(),
            content_type: default_content_type(),
            output_dir: String::new(),
            default_container: default_container(),
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let Some(path) = config_file() else {
            return Self::default();
        };
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| toml::from_str(&text).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = config_file() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = toml::to_string_pretty(self) {
            let _ = std::fs::write(path, text);
        }
    }
}

pub fn config_dir() -> Option<PathBuf> {
    // Unit and snapshot tests must never read or write the real user settings.
    if cfg!(test) {
        return None;
    }
    fotogramma_core::paths::config_dir()
}

/// Where the scan cache lives. None in tests, like the settings.
pub fn cache_file() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("scan-cache.json"))
}

/// Where a running queue is kept. None in tests.
pub fn queue_file() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("queue.json"))
}

pub fn config_file() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("settings.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_toml() {
        let settings = Settings {
            directory: "/Filmati".to_string(),
            window_width: Some(1280.0),
            window_height: Some(820.0),
            language: "en".to_string(),
            check_updates: false,
            update_choice_made: true,
            presets: vec![SavedPreset {
                name: "Anime 1080p".to_string(),
                crf: 20,
                preset: "slow".to_string(),
                pixel: "yuv420p10le".to_string(),
                tune: "animation".to_string(),
                aq: Some(3),
                audio_index: 2,
                container: "mkv".to_string(),
            }],
            scan_cache: false,
            prevent_sleep: false,
            min_saving_percent: 15,
            stop_if_larger: false,
            skipped_update: "v9.9.9".to_string(),
            accepted_codecs: vec!["h264".to_string()],
            replace_original: true,
            default_profile: "fast".to_string(),
            content_type: "anime".to_string(),
            output_dir: "/Volumes/Out".to_string(),
            default_container: "mp4".to_string(),
        };
        let text = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&text).unwrap();
        assert_eq!(parsed.directory, "/Filmati");
        assert_eq!(parsed.window_width, Some(1280.0));
        assert_eq!(parsed.accepted_codecs, vec!["h264".to_string()]);
        assert!(parsed.replace_original);
        assert_eq!(parsed.default_container, "mp4");
        assert_eq!(parsed.content_type, "anime");
        assert_eq!(parsed.output_dir, "/Volumes/Out");
        assert_eq!(parsed.language, "en");
        assert!(!parsed.check_updates);
        assert!(!parsed.prevent_sleep);
        assert!(!parsed.scan_cache);
        assert_eq!(parsed.presets.len(), 1);
        assert_eq!(parsed.presets[0].name, "Anime 1080p");
        assert_eq!(parsed.presets[0].aq, Some(3));
        assert_eq!(parsed.min_saving_percent, 15);
        assert!(!parsed.stop_if_larger);
        assert_eq!(parsed.skipped_update, "v9.9.9");
    }

    #[test]
    fn missing_fields_get_defaults() {
        let parsed: Settings = toml::from_str("directory = \"/tmp\"\n").unwrap();
        assert_eq!(parsed.directory, "/tmp");
        assert_eq!(parsed.window_width, None);
        assert_eq!(parsed.accepted_codecs, vec!["hevc", "av1"]);
        assert!(!parsed.replace_original);
        assert_eq!(parsed.default_profile, "balanced");
        assert_eq!(parsed.default_container, "mkv");
        assert_eq!(parsed.content_type, "auto");
        assert!(parsed.output_dir.is_empty());
        assert_eq!(parsed.language, "auto");
        assert!(
            !parsed.check_updates,
            "no network request until the user agrees"
        );
        assert!(!parsed.update_choice_made);
        assert!(parsed.prevent_sleep);
        assert!(parsed.scan_cache);
        assert!(parsed.presets.is_empty());
        assert_eq!(parsed.min_saving_percent, 0);
        assert!(parsed.stop_if_larger, "on unless switched off");
        assert!(parsed.skipped_update.is_empty());
    }
}
