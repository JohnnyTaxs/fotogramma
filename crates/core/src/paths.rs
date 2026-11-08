//! Where Fotogramma keeps its files, shared by the app and the command line.

use std::path::PathBuf;

/// `~/Library/Application Support/Fotogramma`, `%APPDATA%\Fotogramma`, or
/// `$XDG_CONFIG_HOME/fotogramma` (`~/.config/fotogramma`).
pub fn config_dir() -> Option<PathBuf> {
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

/// The scan cache, shared: a scan from the command line warms the app's.
pub fn scan_cache_file() -> Option<PathBuf> {
    config_dir().map(|directory| directory.join("scan-cache.json"))
}
