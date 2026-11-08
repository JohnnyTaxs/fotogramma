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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub directory: String,
    #[serde(default)]
    pub window_width: Option<f32>,
    #[serde(default)]
    pub window_height: Option<f32>,
    /// Interface language: `auto` (follow the system), `it` or `en`.
    #[serde(default = "default_language")]
    pub language: String,
    /// Codecs treated as already compatible.
    #[serde(default = "default_accepted_codecs")]
    pub accepted_codecs: Vec<String>,
    /// Pre-select "replace the original" in the conversion dialog.
    #[serde(default)]
    pub replace_original: bool,
    /// Conversion profile key pre-selected in the conversion dialog.
    #[serde(default = "default_profile")]
    pub default_profile: String,
    /// Output container for new files: `mkv` or `mp4`.
    #[serde(default = "default_container")]
    pub default_container: String,
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
            accepted_codecs: default_accepted_codecs(),
            replace_original: false,
            default_profile: default_profile(),
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
    if cfg!(target_os = "macos") {
        let home = std::env::var_os("HOME")?;
        return Some(PathBuf::from(home).join("Library/Application Support/Fotogramma"));
    }
    if cfg!(windows) {
        let appdata = std::env::var_os("APPDATA")?;
        return Some(PathBuf::from(appdata).join("Fotogramma"));
    }
    if let Some(config) = std::env::var_os("XDG_CONFIG_HOME") {
        if !config.is_empty() {
            return Some(PathBuf::from(config).join("fotogramma"));
        }
    }
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".config/fotogramma"))
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
            accepted_codecs: vec!["h264".to_string()],
            replace_original: true,
            default_profile: "fast".to_string(),
            default_container: "mp4".to_string(),
        };
        let text = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&text).unwrap();
        assert_eq!(parsed.directory, "/Filmati");
        assert_eq!(parsed.window_width, Some(1280.0));
        assert_eq!(parsed.accepted_codecs, vec!["h264".to_string()]);
        assert!(parsed.replace_original);
        assert_eq!(parsed.default_container, "mp4");
        assert_eq!(parsed.language, "en");
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
        assert_eq!(parsed.language, "auto");
    }
}
