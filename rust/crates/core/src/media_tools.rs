//! Locate ffmpeg and ffprobe in source, packaged and system installations.

use crate::scanner::process_command;
use std::path::{Path, PathBuf};
use std::process::Stdio;

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

pub fn executable_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Candidate directories, in order: explicit override, application directory,
/// the macOS bundle resource folder, and the current working directory.
pub fn tool_directories() -> Vec<PathBuf> {
    let mut directories = Vec::new();
    if let Ok(value) = std::env::var("FOTOGRAMMA_FFMPEG_DIR") {
        if !value.trim().is_empty() {
            directories.push(PathBuf::from(value));
        }
    }
    let executable = executable_dir();
    directories.push(executable.clone());
    directories.push(executable.join("bin"));
    directories.push(executable.join("..").join("Resources").join("bin"));
    directories.push(executable.join("..").join("Resources"));
    if let Ok(cwd) = std::env::current_dir() {
        directories.push(cwd);
    }
    directories
}

pub fn find_tool(name: &str) -> Result<PathBuf, String> {
    let file_name = executable_name(name);
    for directory in tool_directories() {
        let candidate = directory.join(&file_name);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(&file_name);
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }
    Err(format!(
        "{name} non trovato. Installa FFmpeg oppure usa la build portabile che lo include."
    ))
}

pub fn ffmpeg_path() -> Result<PathBuf, String> {
    find_tool("ffmpeg")
}

pub fn ffprobe_path() -> Result<PathBuf, String> {
    find_tool("ffprobe")
}

/// When the binaries sit next to their own dylibs (macOS portable builds),
/// point the dynamic loader at that directory for child processes.
pub fn tool_environment(tool: &Path) -> Vec<(String, String)> {
    if !cfg!(target_os = "macos") {
        return Vec::new();
    }
    let Some(directory) = tool.parent() else {
        return Vec::new();
    };
    let has_libraries = std::fs::read_dir(directory)
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry
                    .path()
                    .extension()
                    .map(|extension| extension == "dylib")
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false);
    if !has_libraries {
        return Vec::new();
    }
    let existing = std::env::var("DYLD_LIBRARY_PATH").unwrap_or_default();
    let value = if existing.is_empty() {
        directory.to_string_lossy().into_owned()
    } else {
        format!("{}:{existing}", directory.to_string_lossy())
    };
    vec![("DYLD_LIBRARY_PATH".to_string(), value)]
}

/// Apply the tool environment to a command that runs a bundled binary.
pub fn prepare_command(tool: &Path) -> std::process::Command {
    let mut command = process_command(tool);
    for (key, value) in tool_environment(tool) {
        command.env(key, value);
    }
    command
}

pub fn ffprobe_version(ffprobe: &Path) -> Option<String> {
    let mut command = prepare_command(ffprobe);
    command
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = command.output().ok()?;
    if !output.status.success() {
        return None;
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.lines().next().map(|line| line.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_names_follow_the_platform() {
        let name = executable_name("ffmpeg");
        if cfg!(windows) {
            assert_eq!(name, "ffmpeg.exe");
        } else {
            assert_eq!(name, "ffmpeg");
        }
    }

    #[test]
    fn tool_directories_start_with_the_executable_folder() {
        let directories = tool_directories();
        assert!(!directories.is_empty());
        assert_eq!(directories[0], executable_dir());
        assert!(directories.contains(&executable_dir().join("bin")));
    }
}
