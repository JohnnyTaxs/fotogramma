//! Lists of videos for other tools: plain paths, CSV, or JSON.

use crate::scanner::VideoInfo;
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Paths,
    Csv,
    Json,
}

impl Format {
    /// `txt` (or anything unknown) is a list of paths.
    pub fn from_extension(extension: &str) -> Format {
        match extension.to_ascii_lowercase().as_str() {
            "csv" => Format::Csv,
            "json" => Format::Json,
            _ => Format::Paths,
        }
    }
}

/// Average video bitrate in kilobits per second, from the file size and the
/// duration: a high value is a file with a lot to gain.
pub fn bitrate_kbps(video: &VideoInfo) -> Option<u64> {
    let (size, duration) = (video.size?, video.duration?);
    (duration > 0.0).then(|| (size as f64 * 8.0 / duration / 1000.0).round() as u64)
}

#[derive(Serialize)]
struct Row<'a> {
    path: String,
    folder: &'a str,
    codec: &'a str,
    category: &'static str,
    width: Option<u32>,
    height: Option<u32>,
    duration_seconds: Option<f64>,
    size_bytes: Option<u64>,
    bitrate_kbps: Option<u64>,
    hdr: bool,
    error: &'a str,
}

fn rows(videos: &[VideoInfo]) -> Vec<Row<'_>> {
    videos
        .iter()
        .map(|video| Row {
            path: video.path.to_string_lossy().into_owned(),
            folder: &video.folder,
            codec: video.codec.as_deref().unwrap_or(""),
            category: video.category.as_str(),
            width: video.width,
            height: video.height,
            duration_seconds: video.duration,
            size_bytes: video.size,
            bitrate_kbps: bitrate_kbps(video),
            hdr: video.hdr,
            error: video.error.as_deref().unwrap_or(""),
        })
        .collect()
}

pub fn to_paths(videos: &[VideoInfo]) -> String {
    let mut text: String = videos
        .iter()
        .map(|video| video.path.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n");
    text.push('\n');
    text
}

pub fn to_json(videos: &[VideoInfo]) -> String {
    serde_json::to_string_pretty(&rows(videos)).unwrap_or_else(|_| "[]".to_string()) + "\n"
}

/// One field of a CSV line: quoted when needed, and defanged when a
/// spreadsheet would read it as a formula.
fn csv_field(text: &str) -> String {
    let formula = text.starts_with(['=', '+', '-', '@', '\t', '\r']);
    let text = if formula {
        format!("'{text}")
    } else {
        text.to_string()
    };
    if text.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", text.replace('"', "\"\""))
    } else {
        text
    }
}

pub fn to_csv(videos: &[VideoInfo]) -> String {
    let mut text = String::from(
        "path,folder,codec,category,width,height,duration_seconds,size_bytes,bitrate_kbps,hdr,error\n",
    );
    let number = |value: Option<String>| value.unwrap_or_default();
    for row in rows(videos) {
        let fields = [
            csv_field(&row.path),
            csv_field(row.folder),
            csv_field(row.codec),
            row.category.to_string(),
            number(row.width.map(|v| v.to_string())),
            number(row.height.map(|v| v.to_string())),
            number(row.duration_seconds.map(|v| format!("{v:.2}"))),
            number(row.size_bytes.map(|v| v.to_string())),
            number(row.bitrate_kbps.map(|v| v.to_string())),
            row.hdr.to_string(),
            csv_field(row.error),
        ];
        text.push_str(&fields.join(","));
        text.push('\n');
    }
    text
}

pub fn render(videos: &[VideoInfo], format: Format) -> String {
    match format {
        Format::Paths => to_paths(videos),
        Format::Csv => to_csv(videos),
        Format::Json => to_json(videos),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scanner::Category;
    use std::path::PathBuf;

    fn video(path: &str, codec: &str, size: u64, duration: f64) -> VideoInfo {
        VideoInfo {
            hdr: false,
            path: PathBuf::from(path),
            relative_path: "a.mkv".into(),
            name: "a.mkv".into(),
            folder: "Show, Season 1".into(),
            extension: "mkv".into(),
            category: Category::Convert,
            codec: Some(codec.into()),
            size: Some(size),
            duration: Some(duration),
            width: Some(1920),
            height: Some(1080),
            error: None,
        }
    }

    #[test]
    fn the_extension_picks_the_format() {
        assert_eq!(Format::from_extension("CSV"), Format::Csv);
        assert_eq!(Format::from_extension("json"), Format::Json);
        assert_eq!(Format::from_extension("txt"), Format::Paths);
        assert_eq!(Format::from_extension(""), Format::Paths);
    }

    #[test]
    fn bitrate_comes_from_size_and_duration() {
        // 1,000,000 bytes over 10 s is 800 kbit/s.
        assert_eq!(
            bitrate_kbps(&video("/a", "h264", 1_000_000, 10.0)),
            Some(800)
        );
        assert_eq!(bitrate_kbps(&video("/a", "h264", 1_000_000, 0.0)), None);
    }

    #[test]
    fn csv_quotes_what_needs_it_and_defuses_formulas() {
        let csv = to_csv(&[video("/lib/=cmd, \"x\".mkv", "h264", 1_000_000, 10.0)]);
        let lines: Vec<&str> = csv.lines().collect();
        assert!(lines[0].starts_with("path,folder,codec"));
        assert_eq!(lines.len(), 2);
        assert!(lines[1].starts_with("\"/lib/=cmd, \"\"x\"\".mkv\",\"Show, Season 1\",h264,convert,1920,1080,10.00,1000000,800,false,"));
        // A path that starts with `=` would be a formula in a spreadsheet.
        let formula = to_csv(&[video("=1+1", "h264", 1, 1.0)]);
        assert!(formula.lines().nth(1).unwrap().starts_with("'=1+1,"));
    }

    #[test]
    fn json_is_a_list_of_rows() {
        let json = to_json(&[video("/lib/a.mkv", "vp9", 2_000, 4.0)]);
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed[0]["codec"], "vp9");
        assert_eq!(parsed[0]["bitrate_kbps"], 4);
        assert_eq!(parsed[0]["category"], "convert");
        assert_eq!(to_json(&[]).trim(), "[]");
    }

    #[test]
    fn paths_are_one_per_line() {
        let text = to_paths(&[
            video("/a.mkv", "h264", 1, 1.0),
            video("/b.mkv", "h264", 1, 1.0),
        ]);
        assert_eq!(text, "/a.mkv\n/b.mkv\n");
        assert_eq!(render(&[], Format::Paths), "\n");
    }
}
