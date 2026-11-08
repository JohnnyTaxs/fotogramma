//! Italian display formatting shared by the interface.

pub fn format_int(value: usize) -> String {
    let digits = value.to_string();
    let mut formatted = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push('.');
        }
        formatted.push(character);
    }
    formatted
}

pub fn format_bytes(value: Option<u64>) -> String {
    let Some(value) = value else {
        return "—".to_string();
    };
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut amount = value as f64;
    let mut index = 0;
    while amount >= 1024.0 && index < units.len() - 1 {
        amount /= 1024.0;
        index += 1;
    }
    if index == 0 {
        format!("{} {}", amount as u64, units[index])
    } else {
        format!("{:.1}", amount).replace('.', ",") + " " + units[index]
    }
}

pub fn format_duration(value: Option<f64>) -> String {
    let Some(value) = value else {
        return "—".to_string();
    };
    let total = value.max(0.0).round() as u64;
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

pub fn codec_label(codec: Option<&str>) -> String {
    let Some(codec) = codec else {
        return "Sconosciuto".to_string();
    };
    match codec {
        "hevc" => "H.265".to_string(),
        "h264" => "H.264".to_string(),
        "av1" => "AV1".to_string(),
        "vp9" => "VP9".to_string(),
        "mpeg4" => "MPEG-4".to_string(),
        other => other.to_uppercase(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_integers_with_italian_thousands() {
        assert_eq!(format_int(0), "0");
        assert_eq!(format_int(1234), "1.234");
        assert_eq!(format_int(1234567), "1.234.567");
    }

    #[test]
    fn formats_bytes_with_italian_decimals() {
        assert_eq!(format_bytes(None), "—");
        assert_eq!(format_bytes(Some(512)), "512 B");
        assert_eq!(format_bytes(Some(1536)), "1,5 KB");
        assert_eq!(format_bytes(Some(45_709_892)), "43,6 MB");
    }

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration(None), "—");
        assert_eq!(format_duration(Some(65.4)), "1:05");
        assert_eq!(format_duration(Some(3725.0)), "1:02:05");
    }

    #[test]
    fn labels_known_codecs() {
        assert_eq!(codec_label(Some("hevc")), "H.265");
        assert_eq!(codec_label(Some("av1")), "AV1");
        assert_eq!(codec_label(None), "Sconosciuto");
        assert_eq!(codec_label(Some("vp8")), "VP8");
    }
}
