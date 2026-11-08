//! Opening folders in the system file manager, without a browser or server.

use std::path::Path;
use std::process::Command;

pub fn open_folder(path: &Path) {
    open_target(&path.to_string_lossy());
}

fn open_target(target: &str) {
    #[cfg(target_os = "macos")]
    let result = Command::new("open").arg(target).spawn();
    #[cfg(target_os = "windows")]
    let result = {
        use std::os::windows::process::CommandExt;
        // Explorer directly, and no console flashing for a `cmd` helper.
        Command::new("explorer")
            .arg(target)
            .creation_flags(0x0800_0000)
            .spawn()
    };
    #[cfg(all(unix, not(target_os = "macos")))]
    let result = Command::new("xdg-open").arg(target).spawn();
    let _ = result;
}
