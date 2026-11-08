//! Interface language: Italian and English.
//!
//! The language is a process-wide setting, because messages are produced on
//! worker threads (verification, conversion) as well as in the interface.
//! Italian is the default; the application sets the real value at startup.

use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    It,
    En,
}

static CURRENT: AtomicU8 = AtomicU8::new(0);

pub fn lang() -> Lang {
    if CURRENT.load(Ordering::Relaxed) == 1 {
        Lang::En
    } else {
        Lang::It
    }
}

pub fn set_lang(lang: Lang) {
    CURRENT.store(
        match lang {
            Lang::It => 0,
            Lang::En => 1,
        },
        Ordering::Relaxed,
    );
}

impl Lang {
    /// Map a locale tag such as `it-IT`, `en_US.UTF-8` or `fr` to a supported
    /// language. Anything that is not Italian is English.
    pub fn from_locale(tag: &str) -> Lang {
        let primary = tag
            .split(['-', '_', '.'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        if primary == "it" {
            Lang::It
        } else {
            Lang::En
        }
    }

    /// Resolve the saved setting: `"it"`, `"en"`, or anything else (`"auto"`)
    /// to follow `system_locale`.
    pub fn resolve(setting: &str, system_locale: Option<&str>) -> Lang {
        match setting {
            "it" => Lang::It,
            "en" => Lang::En,
            _ => system_locale.map(Lang::from_locale).unwrap_or(Lang::En),
        }
    }
}

/// Pick a static string for the current language.
pub fn tr_str(it: &'static str, en: &'static str) -> &'static str {
    match lang() {
        Lang::It => it,
        Lang::En => en,
    }
}

/// Format a message in the current language. Both templates take the same
/// arguments: `tr!("{n} file", "{n} files", n = count)`.
#[macro_export]
macro_rules! tr {
    ($it:literal, $en:literal $(, $($arg:tt)*)?) => {
        match $crate::i18n::lang() {
            $crate::i18n::Lang::It => format!($it $(, $($arg)*)?),
            $crate::i18n::Lang::En => format!($en $(, $($arg)*)?),
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locales_map_to_languages() {
        assert_eq!(Lang::from_locale("it-IT"), Lang::It);
        assert_eq!(Lang::from_locale("it_CH.UTF-8"), Lang::It);
        assert_eq!(Lang::from_locale("en-US"), Lang::En);
        assert_eq!(Lang::from_locale("fr-FR"), Lang::En);
        assert_eq!(Lang::from_locale(""), Lang::En);
    }

    #[test]
    fn the_setting_overrides_the_system_language() {
        assert_eq!(Lang::resolve("auto", Some("it-IT")), Lang::It);
        assert_eq!(Lang::resolve("auto", Some("de-DE")), Lang::En);
        assert_eq!(Lang::resolve("auto", None), Lang::En);
        assert_eq!(Lang::resolve("en", Some("it-IT")), Lang::En);
        assert_eq!(Lang::resolve("it", Some("en-US")), Lang::It);
        assert_eq!(Lang::resolve("garbage", Some("it")), Lang::It);
    }
}
