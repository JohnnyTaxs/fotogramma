//! Display formatting shared by the interface: Italian uses `.` for thousands
//! and `,` for decimals, English the opposite.

use crate::i18n::{lang, tr_str, Lang};

pub fn format_int(value: usize) -> String {
    let digits = value.to_string();
    let mut formatted = String::new();
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            formatted.push(if lang() == Lang::It { '.' } else { ',' });
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
        let number = format!("{amount:.1}");
        let number = if lang() == Lang::It {
            number.replace('.', ",")
        } else {
            number
        };
        format!("{number} {}", units[index])
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

/// How much room a conversion freed (or took) in total.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpaceChange {
    pub before: u64,
    pub after: u64,
}

impl SpaceChange {
    pub fn add(&mut self, before: u64, after: u64) {
        self.before += before;
        self.after += after;
    }

    /// True when the result is not larger than the original.
    pub fn is_saving(&self) -> bool {
        self.after <= self.before
    }

    /// Bytes saved, or bytes added when the result grew.
    pub fn delta(&self) -> u64 {
        self.before.abs_diff(self.after)
    }

    /// The change as a whole percentage of the original size.
    pub fn percent(&self) -> u32 {
        if self.before == 0 {
            return 0;
        }
        (self.delta() as f64 / self.before as f64 * 100.0).round() as u32
    }

    /// `-60%` when smaller, `+12%` when larger.
    pub fn signed_percent(&self) -> String {
        let sign = if self.is_saving() { '-' } else { '+' };
        format!("{sign}{}%", self.percent())
    }
}

pub fn codec_label(codec: Option<&str>) -> String {
    let Some(codec) = codec else {
        return tr_str("Sconosciuto", "Unknown").to_string();
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
    fn space_change_reports_savings_and_growth() {
        let mut total = SpaceChange::default();
        total.add(2_000, 800);
        total.add(1_000, 400);
        assert!(total.is_saving());
        assert_eq!(total.delta(), 1_800);
        assert_eq!(total.percent(), 60);
        assert_eq!(total.signed_percent(), "-60%");

        let grown = SpaceChange {
            before: 1_000,
            after: 1_120,
        };
        assert!(!grown.is_saving());
        assert_eq!(grown.delta(), 120);
        assert_eq!(grown.signed_percent(), "+12%");

        assert_eq!(SpaceChange::default().percent(), 0);
        assert!(SpaceChange {
            before: 5,
            after: 5
        }
        .is_saving());
    }

    #[test]
    fn labels_known_codecs() {
        assert_eq!(codec_label(Some("hevc")), "H.265");
        assert_eq!(codec_label(Some("av1")), "AV1");
        assert_eq!(codec_label(None), "Sconosciuto");
        assert_eq!(codec_label(Some("vp8")), "VP8");
    }
}
