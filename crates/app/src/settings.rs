//! Small persisted preferences: directory and window size only.
//! Scan results are never stored.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub directory: String,
    #[serde(default)]
    pub window_width: Option<f32>,
    #[serde(default)]
    pub window_height: Option<f32>,
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
        };
        let text = toml::to_string_pretty(&settings).unwrap();
        let parsed: Settings = toml::from_str(&text).unwrap();
        assert_eq!(parsed.directory, "/Filmati");
        assert_eq!(parsed.window_width, Some(1280.0));
    }

    #[test]
    fn missing_fields_get_defaults() {
        let parsed: Settings = toml::from_str("directory = \"/tmp\"\n").unwrap();
        assert_eq!(parsed.directory, "/tmp");
        assert_eq!(parsed.window_width, None);
    }
}
