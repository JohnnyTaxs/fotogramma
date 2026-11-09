//! Self-update: check GitHub for a newer release, download it, verify it, and
//! swap the installed copy once the application has quit.
//!
//! This is the only module that talks to the network. It contacts exactly two
//! hosts through HTTPS: `api.github.com` (the release description) and the
//! release download URLs of this repository. Nothing about the user's library
//! is ever sent. A download is installed only if its SHA-256 matches the
//! `SHA256SUMS.txt` published next to it.

use crate::tr;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

pub const REPOSITORY: &str = "JohnnyTaxs/fotogramma";
const SUMS_NAME: &str = "SHA256SUMS.txt";
/// A release package is never larger than this; anything bigger is refused.
const MAX_PACKAGE_BYTES: u64 = 200_000_000;

fn download_prefix() -> String {
    format!("https://github.com/{REPOSITORY}/releases/download/")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    pub url: String,
    pub size: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub tag: String,
    pub version: String,
    pub notes: String,
    pub page_url: String,
    /// The archive for this platform, if the release has one.
    pub package: Option<Package>,
    pub sums_url: Option<String>,
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// `v2.3.0`, `2.3` or `2.3.0-beta` → `(2, 3, 0)`.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let core = text.trim().trim_start_matches('v');
    let core = core.split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

pub fn is_newer(current: &str, candidate: &str) -> bool {
    match (parse_version(current), parse_version(candidate)) {
        (Some(current), Some(candidate)) => candidate > current,
        _ => false,
    }
}

/// Name of the archive built for this operating system and architecture, as
/// published by `deployment/build_rust.py`.
pub fn package_name() -> Option<String> {
    let system = match std::env::consts::OS {
        "macos" => "Darwin",
        "linux" => "Linux",
        "windows" => "Windows",
        _ => return None,
    };
    let machine = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "arm64",
        ("windows", "x86_64") => "amd64",
        (_, "x86_64") => "x86_64",
        _ => return None,
    };
    Some(format!("Fotogramma-{system}-{machine}.zip"))
}

/// Interpret the GitHub release JSON. `None` means "nothing newer".
pub fn pick_release(json: &str, current: &str, package: &str) -> Result<Option<Release>, String> {
    let release: ApiRelease = serde_json::from_str(json).map_err(|error| {
        tr!(
            "Risposta di GitHub non valida: {error}",
            "Invalid response from GitHub: {error}"
        )
    })?;
    if release.draft || release.prerelease || !is_newer(current, &release.tag_name) {
        return Ok(None);
    }
    let prefix = download_prefix();
    let trusted = |asset: &&ApiAsset| asset.browser_download_url.starts_with(&prefix);
    let found = release
        .assets
        .iter()
        .filter(trusted)
        .find(|asset| asset.name == package)
        .map(|asset| Package {
            name: asset.name.clone(),
            url: asset.browser_download_url.clone(),
            size: asset.size,
        });
    let sums_url = release
        .assets
        .iter()
        .filter(trusted)
        .find(|asset| asset.name == SUMS_NAME)
        .map(|asset| asset.browser_download_url.clone());
    Ok(Some(Release {
        version: release.tag_name.trim_start_matches('v').to_string(),
        tag: release.tag_name,
        notes: release.body.unwrap_or_default().trim().to_string(),
        page_url: release.html_url,
        package: found,
        sums_url,
    }))
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(format!("Fotogramma/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

fn network_error(error: impl std::fmt::Display) -> String {
    tr!(
        "Rete non disponibile: {error}",
        "Network unavailable: {error}"
    )
}

/// Ask GitHub for the latest release. `Ok(None)` when this copy is current.
pub fn check(current: &str) -> Result<Option<Release>, String> {
    let Some(package) = package_name() else {
        return Ok(None);
    };
    let url = format!("https://api.github.com/repos/{REPOSITORY}/releases/latest");
    let mut response = agent(Duration::from_secs(10))
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(network_error)?;
    let body = response
        .body_mut()
        .read_to_string()
        .map_err(network_error)?;
    pick_release(&body, current, &package)
}

/// Find the hash of `name` in a `sha256sum`-style file.
pub fn parse_sums(text: &str, name: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let mut parts = line.split_whitespace();
        let hash = parts.next()?;
        let file = parts.next()?.trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

pub fn sha256_file(path: &Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Download the package for this platform into `directory` and verify it.
/// `progress` receives `(bytes so far, expected total)`.
pub fn download(
    release: &Release,
    directory: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    let package = release.package.as_ref().ok_or_else(|| {
        tr!(
            "Questa versione non ha un pacchetto per il tuo sistema.",
            "This release has no package for your system."
        )
    })?;
    if package.size > MAX_PACKAGE_BYTES {
        return Err(tr!("Pacchetto troppo grande.", "The package is too large."));
    }
    let sums_url = release.sums_url.as_ref().ok_or_else(|| {
        tr!(
            "La release non pubblica i checksum: aggiornamento rifiutato.",
            "The release publishes no checksums: update refused."
        )
    })?;
    let sums = agent(Duration::from_secs(20))
        .get(sums_url)
        .call()
        .map_err(network_error)?
        .body_mut()
        .read_to_string()
        .map_err(network_error)?;
    let expected = parse_sums(&sums, &package.name).ok_or_else(|| {
        tr!(
            "Checksum del pacchetto non trovato.",
            "Checksum for the package not found."
        )
    })?;

    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let target = directory.join(&package.name);
    let mut response = agent(Duration::from_secs(900))
        .get(&package.url)
        .call()
        .map_err(network_error)?;
    let mut reader = response
        .body_mut()
        .with_config()
        .limit(MAX_PACKAGE_BYTES)
        .reader();
    let mut file = fs::File::create(&target).map_err(|error| error.to_string())?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = fs::remove_file(&target);
            return Err(tr!("Download interrotto.", "Download cancelled."));
        }
        let read = reader.read(&mut buffer).map_err(network_error)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
        file.write_all(&buffer[..read])
            .map_err(|error| error.to_string())?;
        done += read as u64;
        progress(done, package.size);
    }
    drop(file);
    if hex(&hasher.finalize()) != expected {
        let _ = fs::remove_file(&target);
        return Err(tr!(
            "Il checksum del download non coincide: aggiornamento rifiutato.",
            "The download checksum does not match: update refused."
        ));
    }
    Ok(target)
}

/// Unpack a release archive with the zip crate, keeping permissions and
/// symbolic links and skipping macOS resource-fork entries.
pub fn extract_zip(archive: &Path, destination: &Path) -> Result<(), String> {
    let file = fs::File::open(archive).map_err(|error| error.to_string())?;
    let mut zip = zip::ZipArchive::new(file).map_err(|error| error.to_string())?;
    for index in 0..zip.len() {
        let mut entry = zip.by_index(index).map_err(|error| error.to_string())?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        if relative
            .components()
            .next()
            .is_some_and(|part| part.as_os_str() == "__MACOSX")
        {
            continue;
        }
        let out = destination.join(&relative);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|error| error.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        #[cfg(unix)]
        if entry.is_symlink() {
            let mut link = String::new();
            entry
                .read_to_string(&mut link)
                .map_err(|error| error.to_string())?;
            std::os::unix::fs::symlink(link, &out).map_err(|error| error.to_string())?;
            continue;
        }
        let mut output = fs::File::create(&out).map_err(|error| error.to_string())?;
        std::io::copy(&mut entry, &mut output).map_err(|error| error.to_string())?;
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o7777))
                .map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// The installed copy that an update replaces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallTarget {
    /// `Fotogramma.app` on macOS, the `Fotogramma` folder elsewhere.
    pub path: PathBuf,
}

impl InstallTarget {
    /// The packaged copy this process runs from, or `None` for a development
    /// binary (`cargo run`), which must never be replaced.
    pub fn detect() -> Option<InstallTarget> {
        Self::from_executable(&std::env::current_exe().ok()?)
    }

    pub fn from_executable(executable: &Path) -> Option<InstallTarget> {
        if cfg!(target_os = "macos") {
            let app = executable
                .ancestors()
                .find(|path| path.extension().is_some_and(|extension| extension == "app"))?;
            return Some(InstallTarget {
                path: app.to_path_buf(),
            });
        }
        let folder = executable.parent()?;
        folder.join("bin").is_dir().then(|| InstallTarget {
            path: folder.to_path_buf(),
        })
    }

    fn staging_directory(&self) -> Option<PathBuf> {
        Some(self.path.parent()?.join(".fotogramma-update"))
    }

    fn staged_name(&self) -> &'static str {
        if cfg!(target_os = "macos") {
            "Fotogramma.app"
        } else {
            "Fotogramma"
        }
    }
}

/// Unpack `archive` next to the installed copy (same filesystem, so the final
/// swap is a rename) and return the new copy. Fails without touching the
/// installed copy when its folder is not writable.
pub fn stage(archive: &Path, target: &InstallTarget) -> Result<PathBuf, String> {
    let staging = target.staging_directory().ok_or_else(|| {
        tr!(
            "Posizione di installazione non valida.",
            "Invalid install location."
        )
    })?;
    let _ = fs::remove_dir_all(&staging);
    fs::create_dir_all(&staging).map_err(|error| {
        tr!(
            "La cartella di installazione non è scrivibile ({error}). Scarica la nuova versione a mano.",
            "The install folder is not writable ({error}). Download the new version by hand."
        )
    })?;
    let result = unpack(archive, &staging).and_then(|()| {
        let staged = staging.join(target.staged_name());
        let executable = if cfg!(target_os = "macos") {
            staged.join("Contents/MacOS/fotogramma")
        } else if cfg!(windows) {
            staged.join("fotogramma.exe")
        } else {
            staged.join("fotogramma")
        };
        if executable.is_file() {
            Ok(staged)
        } else {
            Err(tr!(
                "Il pacchetto scaricato non contiene l'applicazione.",
                "The downloaded package does not contain the application."
            ))
        }
    });
    match result {
        Ok(staged) => {
            verify_signature(&staged)?;
            Ok(staged)
        }
        Err(error) => {
            let _ = fs::remove_dir_all(&staging);
            Err(error)
        }
    }
}

fn unpack(archive: &Path, destination: &Path) -> Result<(), String> {
    if cfg!(target_os = "macos") {
        // ditto keeps symbolic links, permissions and the code signature.
        let status = Command::new("ditto")
            .arg("-x")
            .arg("-k")
            .arg(archive)
            .arg(destination)
            .status()
            .map_err(|error| error.to_string())?;
        return status
            .success()
            .then_some(())
            .ok_or_else(|| tr!("Estrazione non riuscita.", "Extraction failed."));
    }
    extract_zip(archive, destination)
}

fn verify_signature(staged: &Path) -> Result<(), String> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let status = Command::new("codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(staged)
        .status()
        .map_err(|error| error.to_string())?;
    status.success().then_some(()).ok_or_else(|| {
        tr!(
            "La firma dell'applicazione scaricata non è valida.",
            "The signature of the downloaded application is not valid."
        )
    })
}

/// Remove a staging folder left behind by an interrupted update.
pub fn clean_staging(target: &InstallTarget) {
    if let Some(staging) = target.staging_directory() {
        let _ = fs::remove_dir_all(staging);
    }
}

#[cfg(unix)]
const SWAP_SCRIPT: &str = r#"#!/bin/sh
# $1 pid to wait for, $2 installed copy, $3 new copy, $4 staging folder,
# $5 "1" to start the new version afterwards.
while kill -0 "$1" 2>/dev/null; do sleep 0.3; done
old="$2.fotogramma-old"
rm -rf "$old"
if mv "$2" "$old"; then
  if mv "$3" "$2"; then
    rm -rf "$old"
  else
    mv "$old" "$2"
  fi
fi
rm -rf "$4"
if [ "$5" = "1" ]; then
  if [ "$(uname)" = "Darwin" ]; then open "$2"; else "$2/fotogramma" >/dev/null 2>&1 & fi
fi
"#;

/// The swap done in Rust, used on Windows (a copy of the program runs it, see
/// `spawn_swap`) and tested everywhere. Windows will not rename a folder while
/// a program inside it runs, so the move is retried until the old process has
/// gone. On failure the installed copy is left as it was.
pub fn apply_swap(
    installed: &Path,
    staged: &Path,
    staging: &Path,
    relaunch: bool,
) -> Result<(), String> {
    apply_swap_with(
        installed,
        staged,
        staging,
        relaunch,
        240,
        Duration::from_millis(250),
    )
}

fn apply_swap_with(
    installed: &Path,
    staged: &Path,
    staging: &Path,
    relaunch: bool,
    tries: u32,
    delay: Duration,
) -> Result<(), String> {
    let mut old_name = installed.as_os_str().to_owned();
    old_name.push(".fotogramma-old");
    let old = PathBuf::from(old_name);
    let _ = fs::remove_dir_all(&old);
    let mut moved = Err(String::new());
    for attempt in 0..tries {
        moved = fs::rename(installed, &old).map_err(|error| error.to_string());
        if moved.is_ok() {
            break;
        }
        if attempt + 1 < tries {
            std::thread::sleep(delay);
        }
    }
    moved?;
    match fs::rename(staged, installed) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&old);
        }
        Err(error) => {
            let _ = fs::rename(&old, installed);
            return Err(error.to_string());
        }
    }
    let _ = fs::remove_dir_all(staging);
    if relaunch {
        let executable = installed.join(format!("fotogramma{}", std::env::consts::EXE_SUFFIX));
        let _ = Command::new(executable)
            .current_dir(std::env::temp_dir())
            .spawn();
    }
    Ok(())
}

/// Entry point of the helper process: `fotogramma --apply-update <pid>
/// <installed> <new copy> <staging> <0|1>`. Returns the exit code.
pub fn run_apply_update(args: &[String]) -> i32 {
    let [_pid, installed, staged, staging, relaunch] = args else {
        return 2;
    };
    match apply_swap(
        Path::new(installed),
        Path::new(staged),
        Path::new(staging),
        relaunch == "1",
    ) {
        Ok(()) => 0,
        Err(error) => {
            let _ = fs::write(
                std::env::temp_dir().join("fotogramma-update.log"),
                format!("The update could not replace {installed}: {error}\nThe new version is in {staged}.\n"),
            );
            1
        }
    }
}

/// Where the Windows helper (a copy of this program) lives while it works.
#[cfg(windows)]
fn helper_path() -> PathBuf {
    std::env::temp_dir().join("fotogramma-update.exe")
}

/// Remove the helper left by an earlier update. Call it at startup.
pub fn clean_helper() {
    #[cfg(windows)]
    let _ = fs::remove_file(helper_path());
}

/// Start the helper that swaps the installed copy for `staged` after this
/// process (`pid`) has exited. The caller must quit right afterwards.
pub fn spawn_swap(
    target: &InstallTarget,
    staged: &Path,
    pid: u32,
    relaunch: bool,
) -> Result<(), String> {
    let staging = target.staging_directory().ok_or_else(|| {
        tr!(
            "Posizione di installazione non valida.",
            "Invalid install location."
        )
    })?;
    let flag = if relaunch { "1" } else { "0" };
    #[cfg(unix)]
    let mut command = {
        use std::os::unix::process::CommandExt;
        let script = staging.join("swap.sh");
        fs::write(&script, SWAP_SCRIPT).map_err(|error| error.to_string())?;
        let mut command = Command::new("/bin/sh");
        command.arg(&script);
        command.process_group(0);
        command.current_dir(std::env::temp_dir());
        command
    };
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        // No script and no shell: a copy of this program, run from the temp
        // folder (the install folder cannot be renamed while it runs).
        let helper = helper_path();
        fs::copy(
            std::env::current_exe().map_err(|error| error.to_string())?,
            &helper,
        )
        .map_err(|error| error.to_string())?;
        let mut command = Command::new(helper);
        command.arg("--apply-update");
        // CREATE_NO_WINDOW
        command.creation_flags(0x0800_0000);
        command.current_dir(std::env::temp_dir());
        command
    };
    command
        .arg(pid.to_string())
        .arg(&target.path)
        .arg(staged)
        .arg(&staging)
        .arg(flag)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = r#"{
        "tag_name": "v2.4.0",
        "body": "  Notes here  ",
        "html_url": "https://github.com/JohnnyTaxs/fotogramma/releases/tag/v2.4.0",
        "draft": false,
        "prerelease": false,
        "assets": [
            {"name": "Fotogramma-Darwin-arm64.zip", "size": 10,
             "browser_download_url": "https://github.com/JohnnyTaxs/fotogramma/releases/download/v2.4.0/Fotogramma-Darwin-arm64.zip"},
            {"name": "SHA256SUMS.txt", "size": 1,
             "browser_download_url": "https://github.com/JohnnyTaxs/fotogramma/releases/download/v2.4.0/SHA256SUMS.txt"},
            {"name": "Fotogramma-Linux-x86_64.zip", "size": 10,
             "browser_download_url": "https://evil.example.com/Fotogramma-Linux-x86_64.zip"}
        ]
    }"#;

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(parse_version("v2.3.0"), Some((2, 3, 0)));
        assert_eq!(parse_version("2.3"), Some((2, 3, 0)));
        assert_eq!(parse_version("2.3.0-beta.1"), Some((2, 3, 0)));
        assert_eq!(parse_version("2.3.0.1"), None);
        assert_eq!(parse_version("latest"), None);
        assert!(is_newer("2.3.0", "v2.3.1"));
        assert!(is_newer("2.3.9", "2.10.0"));
        assert!(!is_newer("2.3.0", "v2.3.0"));
        assert!(!is_newer("2.3.0", "v2.2.9"));
        assert!(!is_newer("2.3.0", "garbage"));
    }

    #[test]
    fn a_newer_release_is_picked_with_its_package() {
        let release = pick_release(RELEASE, "2.3.0", "Fotogramma-Darwin-arm64.zip")
            .unwrap()
            .expect("newer release");
        assert_eq!(release.version, "2.4.0");
        assert_eq!(release.tag, "v2.4.0");
        assert_eq!(release.notes, "Notes here");
        assert_eq!(release.package.unwrap().name, "Fotogramma-Darwin-arm64.zip");
        assert!(release.sums_url.is_some());
    }

    #[test]
    fn the_current_version_needs_no_update() {
        assert!(
            pick_release(RELEASE, "2.4.0", "Fotogramma-Darwin-arm64.zip")
                .unwrap()
                .is_none()
        );
        assert!(
            pick_release(RELEASE, "3.0.0", "Fotogramma-Darwin-arm64.zip")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn packages_from_other_hosts_are_never_trusted() {
        let release = pick_release(RELEASE, "2.3.0", "Fotogramma-Linux-x86_64.zip")
            .unwrap()
            .unwrap();
        assert!(release.package.is_none());
    }

    #[test]
    fn drafts_and_prereleases_are_ignored() {
        let draft = RELEASE.replace("\"draft\": false", "\"draft\": true");
        assert!(pick_release(&draft, "2.3.0", "x").unwrap().is_none());
        let pre = RELEASE.replace("\"prerelease\": false", "\"prerelease\": true");
        assert!(pick_release(&pre, "2.3.0", "x").unwrap().is_none());
    }

    #[test]
    fn garbage_json_is_an_error() {
        assert!(pick_release("not json", "2.3.0", "x").is_err());
    }

    #[test]
    fn checksums_are_read_from_a_sha256sum_file() {
        let hash = "a".repeat(64);
        let text = format!(
            "{hash}  Fotogramma-Darwin-arm64.zip\n{}  *other.zip\n",
            "b".repeat(64)
        );
        assert_eq!(parse_sums(&text, "Fotogramma-Darwin-arm64.zip"), Some(hash));
        assert_eq!(parse_sums(&text, "other.zip"), Some("b".repeat(64)));
        assert_eq!(parse_sums(&text, "missing.zip"), None);
        assert_eq!(parse_sums("short  file.zip", "file.zip"), None);
    }

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("foto-update-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn sha256_matches_a_known_digest() {
        let directory = scratch("sha");
        let file = directory.join("abc");
        fs::write(&file, b"abc").unwrap();
        assert_eq!(
            sha256_file(&file).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn zip_extraction_keeps_modes_and_skips_resource_forks() {
        use zip::write::SimpleFileOptions;
        let directory = scratch("zip");
        let archive = directory.join("a.zip");
        {
            let mut writer = zip::ZipWriter::new(fs::File::create(&archive).unwrap());
            let options = SimpleFileOptions::default().unix_permissions(0o755);
            writer.start_file("Fotogramma/fotogramma", options).unwrap();
            writer.write_all(b"binary").unwrap();
            writer
                .start_file("__MACOSX/._junk", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"junk").unwrap();
            writer
                .start_file("../escape.txt", SimpleFileOptions::default())
                .unwrap();
            writer.write_all(b"no").unwrap();
            writer.finish().unwrap();
        }
        let out = directory.join("out");
        extract_zip(&archive, &out).unwrap();
        let binary = out.join("Fotogramma/fotogramma");
        assert_eq!(fs::read(&binary).unwrap(), b"binary");
        assert!(!out.join("__MACOSX").exists());
        assert!(!directory.join("escape.txt").exists());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&binary).unwrap().permissions().mode() & 0o777,
                0o755
            );
        }
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn development_binaries_are_never_an_install_target() {
        assert!(
            InstallTarget::from_executable(Path::new("/work/target/release/fotogramma")).is_none()
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_bundled_executable_targets_its_app() {
        let target = InstallTarget::from_executable(Path::new(
            "/Applications/Fotogramma.app/Contents/MacOS/fotogramma",
        ))
        .unwrap();
        assert_eq!(target.path, Path::new("/Applications/Fotogramma.app"));
    }

    #[cfg(unix)]
    #[test]
    fn the_swap_script_replaces_the_copy_and_cleans_up() {
        let directory = scratch("swap");
        let installed = directory.join("Fotogramma");
        let staging = directory.join(".fotogramma-update");
        let staged = staging.join("Fotogramma");
        fs::create_dir_all(&installed).unwrap();
        fs::create_dir_all(&staged).unwrap();
        fs::write(installed.join("version"), "old").unwrap();
        fs::write(staged.join("version"), "new").unwrap();
        let target = InstallTarget {
            path: installed.clone(),
        };
        // No process has this id, so the script does not wait.
        spawn_swap(&target, &staged, 999_999, false).unwrap();
        for _ in 0..100 {
            if !staging.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            fs::read_to_string(installed.join("version")).unwrap(),
            "new"
        );
        assert!(!directory.join("Fotogramma.fotogramma-old").exists());
        assert!(!staging.exists());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn apply_swap_replaces_the_copy_and_cleans_up() {
        let directory = scratch("apply");
        let installed = directory.join("Fotogramma");
        let staging = directory.join(".fotogramma-update");
        let staged = staging.join("Fotogramma");
        fs::create_dir_all(&installed).unwrap();
        fs::create_dir_all(&staged).unwrap();
        fs::write(installed.join("version"), "old").unwrap();
        fs::write(staged.join("version"), "new").unwrap();
        apply_swap_with(&installed, &staged, &staging, false, 2, Duration::ZERO).unwrap();
        assert_eq!(
            fs::read_to_string(installed.join("version")).unwrap(),
            "new"
        );
        assert!(!directory.join("Fotogramma.fotogramma-old").exists());
        assert!(!staging.exists());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn apply_swap_undoes_the_first_move_when_the_second_fails() {
        let directory = scratch("apply-fail");
        let installed = directory.join("Fotogramma");
        fs::create_dir_all(&installed).unwrap();
        fs::write(installed.join("version"), "old").unwrap();
        let staging = directory.join(".fotogramma-update");
        fs::create_dir_all(&staging).unwrap();
        let result = apply_swap_with(
            &installed,
            &staging.join("Missing"),
            &staging,
            false,
            2,
            Duration::ZERO,
        );
        assert!(result.is_err());
        assert_eq!(
            fs::read_to_string(installed.join("version")).unwrap(),
            "old"
        );
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn apply_swap_gives_up_when_the_folder_cannot_be_moved() {
        let directory = scratch("apply-none");
        let result = apply_swap_with(
            &directory.join("Absent"),
            &directory.join("New"),
            &directory.join("staging"),
            false,
            2,
            Duration::ZERO,
        );
        assert!(result.is_err());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn the_helper_arguments_are_checked() {
        assert_eq!(run_apply_update(&["1".to_string()]), 2);
    }

    #[cfg(unix)]
    #[test]
    fn a_failed_swap_keeps_the_installed_copy() {
        let directory = scratch("swap-fail");
        let installed = directory.join("Fotogramma");
        fs::create_dir_all(&installed).unwrap();
        fs::write(installed.join("version"), "old").unwrap();
        let staging = directory.join(".fotogramma-update");
        fs::create_dir_all(&staging).unwrap();
        let target = InstallTarget {
            path: installed.clone(),
        };
        // The new copy does not exist, so the second move fails and the first
        // one must be undone.
        spawn_swap(&target, &staging.join("Missing"), 999_999, false).unwrap();
        for _ in 0..100 {
            if !staging.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert_eq!(
            fs::read_to_string(installed.join("version")).unwrap(),
            "old"
        );
        let _ = fs::remove_dir_all(&directory);
    }

    /// Talks to the real GitHub API: `cargo test -p fotogramma-core live -- --ignored`.
    #[test]
    #[ignore]
    fn live_check_against_github() {
        let release = check("0.0.1").unwrap().expect("a release newer than 0.0.1");
        println!("{} package: {:?}", release.tag, release.package);
        assert!(release.page_url.starts_with("https://github.com/"));
        assert!(check("999.0.0").unwrap().is_none());
    }

    /// Stage and swap a real package: `FOTOGRAMMA_TEST_ZIP=dist/Fotogramma-Darwin-arm64.zip
    /// cargo test -p fotogramma-core real_package -- --ignored` (macOS).
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn real_package_stages_and_swaps() {
        let zip = PathBuf::from(std::env::var("FOTOGRAMMA_TEST_ZIP").expect("FOTOGRAMMA_TEST_ZIP"));
        let directory = scratch("real");
        let installed = directory.join("Fotogramma.app");
        fs::create_dir_all(installed.join("Contents")).unwrap();
        fs::write(installed.join("Contents/old"), "old").unwrap();
        let target = InstallTarget {
            path: installed.clone(),
        };
        let staged = stage(&zip, &target).expect("stage the real package");
        assert!(staged.join("Contents/MacOS/fotogramma").is_file());
        spawn_swap(&target, &staged, 999_999, false).unwrap();
        for _ in 0..200 {
            if !directory.join(".fotogramma-update").exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(installed.join("Contents/MacOS/fotogramma").is_file());
        assert!(!installed.join("Contents/old").exists());
        let status = Command::new("codesign")
            .args(["--verify", "--deep", "--strict"])
            .arg(&installed)
            .status()
            .unwrap();
        assert!(status.success());
        let _ = fs::remove_dir_all(&directory);
    }
}
