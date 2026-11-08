//! The queue of a running conversion, saved to disk so that it can be resumed
//! after the app was closed or the computer lost power.
//!
//! The file exists only while a queue runs: it is rewritten after each file
//! and removed when the queue ends or is stopped on purpose.

use crate::conversion::ConversionProfile;
use crate::scanner::VideoInfo;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const VERSION: u32 = 1;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PendingQueue {
    version: u32,
    /// What is left to convert, in order.
    pub videos: Vec<VideoInfo>,
    pub profile: ConversionProfile,
    pub destination_dir: PathBuf,
    pub container: String,
    pub replace_original: bool,
    pub min_saving_percent: u8,
}

impl PendingQueue {
    pub fn new(
        videos: Vec<VideoInfo>,
        profile: ConversionProfile,
        destination_dir: PathBuf,
        container: String,
        replace_original: bool,
        min_saving_percent: u8,
    ) -> Self {
        Self {
            version: VERSION,
            videos,
            profile,
            destination_dir,
            container,
            replace_original,
            min_saving_percent,
        }
    }

    /// A missing, unreadable, outdated or empty file means no queue.
    pub fn load(path: &Path) -> Option<Self> {
        let queue: Self = serde_json::from_str(&fs::read_to_string(path).ok()?).ok()?;
        (queue.version == VERSION && !queue.videos.is_empty()).then_some(queue)
    }

    /// Videos whose file is still there: one that was moved or deleted since
    /// cannot be resumed.
    pub fn still_present(mut self) -> Self {
        self.videos.retain(|video| video.path.is_file());
        self
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec(self).map_err(std::io::Error::other)?,
        )?;
        fs::rename(&temporary, path)
    }

    pub fn remove(path: &Path) {
        let _ = fs::remove_file(path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::profile;
    use crate::scanner::Category;

    fn video(path: &Path) -> VideoInfo {
        VideoInfo {
            hdr: false,
            path: path.to_path_buf(),
            relative_path: "a.mkv".into(),
            name: "a.mkv".into(),
            folder: String::new(),
            extension: "mkv".into(),
            category: Category::Convert,
            codec: Some("h264".into()),
            size: Some(1),
            duration: Some(1.0),
            width: None,
            height: None,
            error: None,
        }
    }

    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("foto-queue-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn a_queue_survives_a_round_trip_and_can_be_removed() {
        let directory = scratch("round");
        let present = directory.join("a.mkv");
        fs::write(&present, b"x").unwrap();
        let queue = PendingQueue::new(
            vec![video(&present), video(&directory.join("gone.mkv"))],
            profile("balanced").unwrap(),
            directory.join("out"),
            "mkv".into(),
            false,
            10,
        );
        let file = directory.join("sub").join("queue.json");
        queue.save(&file).unwrap();
        assert!(!file.with_extension("json.tmp").exists());

        let loaded = PendingQueue::load(&file).expect("a queue");
        assert_eq!(loaded.videos.len(), 2);
        assert_eq!(loaded.profile, profile("balanced").unwrap());
        assert_eq!(loaded.min_saving_percent, 10);
        // A file that vanished since cannot be resumed.
        let resumable = loaded.still_present();
        assert_eq!(resumable.videos.len(), 1);
        assert_eq!(resumable.videos[0].path, present);

        PendingQueue::remove(&file);
        assert!(PendingQueue::load(&file).is_none());
        let _ = fs::remove_dir_all(&directory);
    }

    #[test]
    fn broken_outdated_or_empty_files_are_no_queue() {
        let directory = scratch("bad");
        let file = directory.join("queue.json");
        fs::write(&file, "not json").unwrap();
        assert!(PendingQueue::load(&file).is_none());
        let mut queue = PendingQueue::new(
            vec![],
            profile("fast").unwrap(),
            "o".into(),
            "mkv".into(),
            false,
            0,
        );
        queue.save(&file).unwrap();
        assert!(PendingQueue::load(&file).is_none(), "nothing left to do");
        queue.videos.push(video(Path::new("/a.mkv")));
        queue.version = 99;
        queue.save(&file).unwrap();
        assert!(PendingQueue::load(&file).is_none(), "another version");
        assert!(PendingQueue::load(&directory.join("missing.json")).is_none());
        let _ = fs::remove_dir_all(&directory);
    }
}
