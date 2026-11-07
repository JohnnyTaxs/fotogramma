//! Development helper: scan a directory and print the summary as JSON.
//!
//! cargo run --example scan -- /path/to/library

use fotogramma_core::media_tools::ffprobe_path;
use fotogramma_core::scanner::{discover_videos, probe_videos, summarize};

fn main() {
    let directory = std::env::args().nth(1).expect("usage: scan <directory>");
    let directory = std::path::PathBuf::from(directory);
    let ffprobe = ffprobe_path().expect("ffprobe");
    let candidates = discover_videos(&directory, None, |_, _| {}, |_, _| {}).expect("discovery");
    let (videos, cancelled) = probe_videos(
        &candidates,
        &directory,
        &ffprobe,
        None,
        |processed, total, current, _| {
            eprintln!("{processed}/{total} {current}");
        },
    );
    let summary = summarize(&videos, None);
    eprintln!("cancelled: {cancelled}");
    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    for video in &videos {
        println!(
            "{}  {:?}  {:?}  {}",
            video.relative_path,
            video.category,
            video.codec,
            video.error.as_deref().unwrap_or("")
        );
    }
}
