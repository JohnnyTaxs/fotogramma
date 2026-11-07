//! Build an external 1337x search URL without any network request in the app.

use std::path::Path;

const RELEASE_TAGS: [&str; 18] = [
    "bluray", "webdl", "webrip", "web-dl", "hdtv", "bdrip", "brrip", "2160p", "1080p", "720p",
    "576p", "480p", "x264", "x265", "h264", "h265", "hevc", "avc",
];

fn is_release_tag(token: &str) -> bool {
    let cleaned: String = token
        .chars()
        .filter(|character| character.is_alphanumeric() || *character == '-')
        .collect::<String>()
        .to_lowercase();
    RELEASE_TAGS.contains(&cleaned.as_str())
}

/// Strip release tags and separators, keeping the human title.
pub fn title_for_search(filename: &str) -> String {
    let stem = Path::new(filename)
        .file_stem()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string());
    let normalized = stem.replace('.', " ").replace('_', " ");
    let mut words = Vec::new();
    for token in normalized.split_whitespace() {
        if is_release_tag(token) {
            break;
        }
        words.push(token);
    }
    let joined = words.join(" ");
    let title = joined.trim_matches(|character| character == '-' || character == '_');
    if title.is_empty() {
        stem
    } else {
        title.to_string()
    }
}

fn quote_plus(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char)
            }
            b' ' => encoded.push('+'),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

pub fn h265_search_url(filename: &str) -> String {
    let title = title_for_search(filename);
    format!(
        "https://1337x.to/search/{}/1/",
        quote_plus(&format!("{title} H.265"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_release_tags() {
        assert_eq!(
            title_for_search("Blade.Runner.2049.2017.1080p.BluRay.x265.mkv"),
            "Blade Runner 2049 2017"
        );
        assert_eq!(title_for_search("Vacanze_al_mare.mkv"), "Vacanze al mare");
    }

    #[test]
    fn keeps_a_plain_title() {
        assert_eq!(title_for_search("Tramonto.mkv"), "Tramonto");
    }

    #[test]
    fn builds_an_encoded_1337x_url() {
        assert_eq!(
            h265_search_url("Blade Runner 2049.mkv"),
            "https://1337x.to/search/Blade+Runner+2049+H.265/1/"
        );
    }
}
