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

/// What to do with the computer when a queue ends.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PowerAction {
    #[default]
    Nothing,
    Sleep,
    Shutdown,
}

impl PowerAction {
    /// The program and arguments that carry the action out on this system.
    pub fn command(self) -> Option<(&'static str, Vec<&'static str>)> {
        if cfg!(target_os = "macos") {
            match self {
                PowerAction::Nothing => None,
                PowerAction::Sleep => Some(("pmset", vec!["sleepnow"])),
                PowerAction::Shutdown => Some((
                    "osascript",
                    vec!["-e", "tell application \"System Events\" to shut down"],
                )),
            }
        } else if cfg!(windows) {
            match self {
                PowerAction::Nothing => None,
                PowerAction::Sleep => Some((
                    "rundll32.exe",
                    vec!["powrprof.dll,SetSuspendState", "0,1,0"],
                )),
                PowerAction::Shutdown => Some(("shutdown", vec!["/s", "/t", "0"])),
            }
        } else {
            match self {
                PowerAction::Nothing => None,
                PowerAction::Sleep => Some(("systemctl", vec!["suspend"])),
                PowerAction::Shutdown => Some(("systemctl", vec!["poweroff"])),
            }
        }
    }

    pub fn run(self) {
        if let Some((program, arguments)) = self.command() {
            let mut command = Command::new(program);
            command.args(arguments);
            #[cfg(windows)]
            {
                use std::os::windows::process::CommandExt;
                command.creation_flags(0x0800_0000);
            }
            let _ = command.spawn();
        }
    }
}

/// Keeps the computer awake while it is alive: a laptop that goes to sleep in
/// the middle of a long conversion simply stops it.
///
/// * macOS: a `caffeinate -i -w <pid>` child, which also ends by itself when
///   the app does.
/// * Linux: a `systemd-inhibit` child that sleeps; without systemd nothing
///   happens.
/// * Windows: `SetThreadExecutionState`, which belongs to the thread that
///   calls it, so create the guard on the thread that does the work.
pub struct SleepGuard {
    #[cfg(not(windows))]
    child: Option<std::process::Child>,
}

impl SleepGuard {
    pub fn acquire(enabled: bool) -> Self {
        #[cfg(windows)]
        {
            if enabled {
                const ES_CONTINUOUS: u32 = 0x8000_0000;
                const ES_SYSTEM_REQUIRED: u32 = 0x0000_0001;
                #[link(name = "kernel32")]
                extern "system" {
                    fn SetThreadExecutionState(flags: u32) -> u32;
                }
                // SAFETY: a plain Win32 call with documented flags.
                unsafe {
                    SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED);
                }
            }
            SleepGuard {}
        }
        #[cfg(not(windows))]
        {
            let child = enabled.then(spawn_inhibitor).flatten();
            SleepGuard { child }
        }
    }

    /// Whether something is really holding the computer awake.
    #[cfg(all(test, not(windows)))]
    pub fn is_active(&self) -> bool {
        self.child.is_some()
    }
}

#[cfg(target_os = "macos")]
fn spawn_inhibitor() -> Option<std::process::Child> {
    Command::new("caffeinate")
        .args(["-i", "-w", &std::process::id().to_string()])
        .stdin(std::process::Stdio::null())
        .spawn()
        .ok()
}

#[cfg(all(unix, not(target_os = "macos")))]
fn spawn_inhibitor() -> Option<std::process::Child> {
    Command::new("systemd-inhibit")
        .args([
            "--what=idle:sleep",
            "--who=Fotogramma",
            "--why=Converting videos",
            "--mode=block",
            "sleep",
            "2147483647",
        ])
        .stdin(std::process::Stdio::null())
        .spawn()
        .ok()
}

impl Drop for SleepGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            #[link(name = "kernel32")]
            extern "system" {
                fn SetThreadExecutionState(flags: u32) -> u32;
            }
            // SAFETY: restores the default state (ES_CONTINUOUS alone).
            unsafe {
                SetThreadExecutionState(0x8000_0000);
            }
        }
        #[cfg(not(windows))]
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn power_actions_map_to_a_command_except_doing_nothing() {
        assert!(PowerAction::Nothing.command().is_none());
        for action in [PowerAction::Sleep, PowerAction::Shutdown] {
            let (program, arguments) = action.command().expect("a command");
            assert!(!program.is_empty() && !arguments.is_empty());
        }
        assert_ne!(
            PowerAction::Sleep.command(),
            PowerAction::Shutdown.command()
        );
    }

    #[test]
    fn a_disabled_guard_holds_nothing() {
        let guard = SleepGuard::acquire(false);
        #[cfg(not(windows))]
        assert!(!guard.is_active());
        drop(guard);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn on_macos_caffeinate_is_held_and_released() {
        let guard = SleepGuard::acquire(true);
        assert!(guard.is_active(), "caffeinate should start");
        drop(guard);
    }
}
