//! Opening external URLs and folders without a browser or server inside the app.

use std::path::Path;
use std::process::Command;

pub fn open_url(url: &str) {
    open_target(url);
}

pub fn open_folder(path: &Path) {
    open_target(&path.to_string_lossy());
}

fn open_target(target: &str) {
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(target).spawn();
    #[cfg(target_os = "windows")]
    let result = Command::new("cmd")
        .args(["/C", "start", "", target])
        .spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(target).spawn();
    let _ = result;
}
