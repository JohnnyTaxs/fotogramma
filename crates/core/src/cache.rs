//! A cache of probe results, so that scanning a library again only probes the
//! files that changed.
//!
//! A file is trusted again when its path, size and modification time are the
//! same as when it was probed. Only readable videos are cached: an unreadable
//! file is probed again every time. The cache never holds anything about the
//! content beyond what the scan shows (codec, size, duration, resolution).

use crate::scanner::{relative_path, Category, VideoInfo};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

// 2: sizes of 4 GiB and more were stored as 4 GiB; the old entries are dropped.
const VERSION: u32 = 2;

#[derive(Clone, Serialize, Deserialize)]
struct Entry {
    size: u64,
    secs: u64,
    nanos: u32,
    video: VideoInfo,
}

#[derive(Serialize, Deserialize)]
struct File {
    version: u32,
    entries: Vec<Entry>,
}

#[derive(Default)]
struct Inner {
    entries: HashMap<PathBuf, Entry>,
    /// Entries looked up or added during this scan.
    seen: HashSet<PathBuf>,
}

/// Safe to share between the probing threads.
#[derive(Default)]
pub struct ScanCache {
    inner: Mutex<Inner>,
}

fn stamp(path: &Path) -> Option<(u64, u64, u32)> {
    let metadata = fs::metadata(path).ok()?;
    let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((metadata.len(), modified.as_secs(), modified.subsec_nanos()))
}

impl ScanCache {
    /// A missing, unreadable or outdated file gives an empty cache.
    pub fn load(path: &Path) -> Self {
        let file: Option<File> = fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok());
        let mut inner = Inner::default();
        if let Some(file) = file.filter(|file| file.version == VERSION) {
            for entry in file.entries {
                inner.entries.insert(entry.video.path.clone(), entry);
            }
        }
        Self {
            inner: Mutex::new(inner),
        }
    }

    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|error| error.into_inner())
    }

    /// The cached result for `file`, if it has not changed since.
    pub fn lookup(&self, file: &Path, root: &Path) -> Option<VideoInfo> {
        let (size, secs, nanos) = stamp(file)?;
        let key = crate::scanner::absolute_path(file);
        let mut inner = self.lock();
        let entry = inner.entries.get(&key)?;
        if (entry.size, entry.secs, entry.nanos) != (size, secs, nanos) {
            return None;
        }
        let mut video = entry.video.clone();
        inner.seen.insert(key);
        video.relative_path = relative_path(file, root);
        video.folder = crate::scanner::folder_of(&video.relative_path);
        Some(video)
    }

    /// Remember a readable video that was just probed.
    pub fn insert(&self, video: &VideoInfo) {
        if video.category == Category::Error {
            return;
        }
        let Some((size, secs, nanos)) = stamp(&video.path) else {
            return;
        };
        let mut inner = self.lock();
        inner.seen.insert(video.path.clone());
        inner.entries.insert(
            video.path.clone(),
            Entry {
                size,
                secs,
                nanos,
                video: video.clone(),
            },
        );
    }

    /// End of a scan of `root`. When it ran to the end, entries under `root`
    /// that were not seen belong to files that are gone: forget them.
    pub fn finish(&self, root: &Path, complete: bool) {
        let mut inner = self.lock();
        if complete {
            let root = crate::scanner::absolute_path(root);
            let seen = std::mem::take(&mut inner.seen);
            inner
                .entries
                .retain(|path, _| !path.starts_with(&root) || seen.contains(path));
        }
        inner.seen.clear();
    }

    /// Write the cache without ever leaving a half-written file behind.
    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        let file = File {
            version: VERSION,
            entries: self.lock().entries.values().cloned().collect(),
        };
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec(&file).map_err(std::io::Error::other)?,
        )?;
        fs::rename(&temporary, path)
    }
}

/// Seconds since the epoch, for tests that set modification times.
pub fn epoch(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("foto-cache-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn video(path: &Path, root: &Path, codec: &str, category: Category) -> VideoInfo {
        let relative = relative_path(path, root);
        VideoInfo {
            hdr: false,
            path: crate::scanner::absolute_path(path),
            folder: crate::scanner::folder_of(&relative),
            relative_path: relative,
            name: path.file_name().unwrap().to_string_lossy().into_owned(),
            extension: "mkv".into(),
            category,
            codec: Some(codec.into()),
            size: Some(5),
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            error: None,
        }
    }

    #[test]
    fn a_probed_file_is_found_again_until_it_changes() {
        let root = scratch("hit");
        let file = root.join("a.mkv");
        fs::write(&file, b"12345").unwrap();
        let cache = ScanCache::default();
        assert!(
            cache.lookup(&file, &root).is_none(),
            "nothing is cached yet"
        );
        cache.insert(&video(&file, &root, "h264", Category::Convert));
        let hit = cache.lookup(&file, &root).expect("a hit");
        assert_eq!(hit.codec.as_deref(), Some("h264"));

        // A different size is a different file.
        fs::write(&file, b"123456789").unwrap();
        assert!(cache.lookup(&file, &root).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_new_modification_time_invalidates_the_entry() {
        let root = scratch("mtime");
        let file = root.join("a.mkv");
        fs::write(&file, b"12345").unwrap();
        let cache = ScanCache::default();
        cache.insert(&video(&file, &root, "h264", Category::Convert));
        assert!(cache.lookup(&file, &root).is_some());
        let later = SystemTime::now() + Duration::from_secs(3600);
        fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(later)
            .unwrap();
        assert!(cache.lookup(&file, &root).is_none());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn unreadable_files_are_never_cached() {
        let root = scratch("error");
        let file = root.join("a.mkv");
        fs::write(&file, b"x").unwrap();
        let cache = ScanCache::default();
        cache.insert(&video(&file, &root, "h264", Category::Error));
        assert!(cache.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_hit_is_relative_to_the_root_of_this_scan() {
        let root = scratch("root");
        fs::create_dir_all(root.join("Show")).unwrap();
        let file = root.join("Show").join("a.mkv");
        fs::write(&file, b"12345").unwrap();
        let cache = ScanCache::default();
        cache.insert(&video(&file, &root, "h264", Category::Convert));
        // The same file seen from its own folder.
        let hit = cache.lookup(&file, &root.join("Show")).unwrap();
        assert_eq!(hit.relative_path, "a.mkv");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn the_cache_survives_a_round_trip_through_a_file() {
        let root = scratch("save");
        let file = root.join("a.mkv");
        fs::write(&file, b"12345").unwrap();
        let cache = ScanCache::default();
        cache.insert(&video(&file, &root, "vp9", Category::Convert));
        let stored = root.join("cache").join("scan-cache.json");
        cache.save(&stored).unwrap();
        assert!(!stored.with_extension("json.tmp").exists());
        let loaded = ScanCache::load(&stored);
        assert_eq!(loaded.len(), 1);
        assert_eq!(
            loaded.lookup(&file, &root).unwrap().codec.as_deref(),
            Some("vp9")
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_broken_or_outdated_file_gives_an_empty_cache() {
        let root = scratch("broken");
        let stored = root.join("scan-cache.json");
        fs::write(&stored, "not json").unwrap();
        assert!(ScanCache::load(&stored).is_empty());
        fs::write(&stored, r#"{"version":99,"entries":[]}"#).unwrap();
        assert!(ScanCache::load(&stored).is_empty());
        assert!(ScanCache::load(&root.join("missing.json")).is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn finishing_a_complete_scan_forgets_files_that_are_gone() {
        let root = scratch("prune");
        let kept = root.join("kept.mkv");
        let gone = root.join("gone.mkv");
        let elsewhere = scratch("prune-other").join("other.mkv");
        for file in [&kept, &gone, &elsewhere] {
            fs::write(file, b"12345").unwrap();
        }
        let cache = ScanCache::default();
        for file in [&kept, &gone, &elsewhere] {
            cache.insert(&video(file, &root, "h264", Category::Convert));
        }
        cache.finish(&root, true); // everything was just inserted: all seen
        assert_eq!(cache.len(), 3);
        // A second scan of `root` that only meets `kept`.
        assert!(cache.lookup(&kept, &root).is_some());
        cache.finish(&root, true);
        assert_eq!(
            cache.len(),
            2,
            "gone.mkv is forgotten, the other root is untouched"
        );
        assert!(cache.lookup(&elsewhere, &root).is_some());

        // An interrupted scan forgets nothing.
        cache.finish(&root, false);
        assert_eq!(cache.len(), 2);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(elsewhere.parent().unwrap());
    }
}
