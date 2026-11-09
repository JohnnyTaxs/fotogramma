//! Cinema Pop theme, rebuilt for egui from the original Qt stylesheet.

use eframe::egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const BG: Color32 = Color32::from_rgb(0x28, 0x15, 0x2f);
pub const BG_DEEP: Color32 = Color32::from_rgb(0x21, 0x11, 0x27);
pub const SURFACE: Color32 = Color32::from_rgb(0x35, 0x1a, 0x40);
pub const SURFACE_ALT: Color32 = Color32::from_rgb(0x2f, 0x18, 0x38);
pub const RAISED: Color32 = Color32::from_rgb(0x48, 0x23, 0x4f);
pub const RAISED_HOVER: Color32 = Color32::from_rgb(0x51, 0x29, 0x5c);
pub const LINE: Color32 = Color32::from_rgb(0x62, 0x40, 0x6b);
pub const INPUT_BORDER: Color32 = Color32::from_rgb(0x70, 0x48, 0x7a);
pub const SELECTION: Color32 = Color32::from_rgb(0x67, 0x39, 0x4e);
pub const TEXT: Color32 = Color32::from_rgb(0xfc, 0xec, 0xf6);
pub const MUTED: Color32 = Color32::from_rgb(0xc7, 0xa9, 0xc5);
pub const CORAL: Color32 = Color32::from_rgb(0xff, 0x94, 0x82);
pub const CORAL_HOVER: Color32 = Color32::from_rgb(0xff, 0xab, 0x98);
pub const CORAL_INK: Color32 = Color32::from_rgb(0x2b, 0x17, 0x2f);
pub const LAVENDER: Color32 = Color32::from_rgb(0xd4, 0xb4, 0xdb);
pub const MINT: Color32 = Color32::from_rgb(0x77, 0xdd, 0xbb);
pub const ERROR: Color32 = Color32::from_rgb(0xff, 0x85, 0x85);
pub const TRACK: Color32 = Color32::from_rgb(0x55, 0x30, 0x5e);
pub const BADGE_BG: Color32 = Color32::from_rgb(0x67, 0x39, 0x4e);
pub const BADGE_TEXT: Color32 = Color32::from_rgb(0xff, 0xd9, 0xd0);

pub const BODY_SIZE: f32 = 13.0;
pub const SMALL_SIZE: f32 = 11.0;
pub const MONO_SIZE: f32 = 10.0;
pub const BRAND_SIZE: f32 = 21.0;
pub const PAGE_TITLE_SIZE: f32 = 28.0;
pub const HERO_SIZE: f32 = 46.0;

pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Monospace)
}

pub fn proportional(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

pub fn install(ctx: &eframe::egui::Context) {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        "outfit".to_string(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../static/fonts/Outfit-Regular.ttf"
        ))),
    );
    fonts.font_data.insert(
        "plex-mono".to_string(),
        Arc::new(FontData::from_static(include_bytes!(
            "../../../static/fonts/IBMPlexMono-Regular.ttf"
        ))),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "outfit".to_string());
    fonts
        .families
        .entry(FontFamily::Monospace)
        .or_default()
        .insert(0, "plex-mono".to_string());
    *BASE_FONTS.lock().unwrap_or_else(|error| error.into_inner()) = Some(fonts.clone());
    ctx.set_fonts(fonts);

    ctx.set_theme(eframe::egui::ThemePreference::Dark);
    ctx.all_styles_mut(|style| {
        style.visuals = visuals();
        style.text_styles = text_styles();
        style.spacing.item_spacing = eframe::egui::vec2(8.0, 8.0);
        style.spacing.button_padding = eframe::egui::vec2(12.0, 8.0);
        style.spacing.interact_size.y = 32.0;
    });
}

fn text_styles() -> BTreeMap<TextStyle, FontId> {
    BTreeMap::from([
        (TextStyle::Small, proportional(SMALL_SIZE)),
        (TextStyle::Body, proportional(BODY_SIZE)),
        (TextStyle::Button, proportional(BODY_SIZE)),
        (TextStyle::Heading, proportional(BRAND_SIZE)),
        (TextStyle::Monospace, mono(MONO_SIZE + 1.0)),
    ])
}

fn visuals() -> Visuals {
    let mut visuals = Visuals::dark();
    visuals.panel_fill = BG;
    visuals.window_fill = SURFACE;
    // Popups and dropdown lists take this outline: a soft purple, not coral.
    visuals.window_stroke = Stroke::new(1.0, INPUT_BORDER);
    visuals.extreme_bg_color = BG_DEEP;
    visuals.faint_bg_color = SURFACE_ALT;
    visuals.override_text_color = Some(TEXT);
    visuals.selection.bg_fill = SELECTION;
    visuals.selection.stroke = Stroke::new(1.0, TEXT);
    visuals.window_corner_radius = CornerRadius::same(14);
    visuals.menu_corner_radius = CornerRadius::same(10);

    let widgets = &mut visuals.widgets;
    widgets.noninteractive.bg_fill = SURFACE;
    widgets.noninteractive.weak_bg_fill = SURFACE;
    widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);

    widgets.inactive.bg_fill = RAISED;
    widgets.inactive.weak_bg_fill = RAISED;
    widgets.inactive.bg_stroke = Stroke::new(1.0, INPUT_BORDER);
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.inactive.corner_radius = CornerRadius::same(8);

    widgets.hovered.bg_fill = RAISED_HOVER;
    widgets.hovered.weak_bg_fill = RAISED_HOVER;
    widgets.hovered.bg_stroke = Stroke::new(1.0, Color32::from_rgb(0x7b, 0x55, 0x83));
    widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.hovered.corner_radius = CornerRadius::same(8);

    widgets.active.bg_fill = CORAL_HOVER;
    widgets.active.weak_bg_fill = CORAL_HOVER;
    widgets.active.bg_stroke = Stroke::new(1.0, CORAL_HOVER);
    // `Visuals::strong_text_color` reuses the active text color, so keep it
    // readable: primary buttons set their own dark ink explicitly.
    widgets.active.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.active.corner_radius = CornerRadius::same(8);

    widgets.open.bg_fill = SURFACE;
    widgets.open.weak_bg_fill = SURFACE;
    widgets.open.bg_stroke = Stroke::new(1.0, INPUT_BORDER);
    widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.open.corner_radius = CornerRadius::same(8);
    visuals
}

/// Shared primary button, matching `#primaryButton` in the Qt theme.
pub fn primary_button(text: impl Into<String>) -> eframe::egui::Button<'static> {
    eframe::egui::Button::new(
        eframe::egui::RichText::new(text.into())
            .color(CORAL_INK)
            .strong(),
    )
    .fill(CORAL)
    .stroke(Stroke::new(1.0, CORAL))
    .corner_radius(CornerRadius::same(8))
}

/// Secondary button used in stat cards and toolbars.
pub fn secondary_button(text: impl Into<String>) -> eframe::egui::Button<'static> {
    eframe::egui::Button::new(eframe::egui::RichText::new(text.into()).color(TEXT))
        .fill(RAISED)
        .stroke(Stroke::new(1.0, INPUT_BORDER))
        .corner_radius(CornerRadius::same(8))
}

pub fn card_frame() -> eframe::egui::Frame {
    eframe::egui::Frame::NONE
        .fill(SURFACE)
        .stroke(Stroke::new(1.0, LINE))
        .corner_radius(CornerRadius::same(16))
        .inner_margin(eframe::egui::Margin::same(18))
}

// ---- fonts for scripts the interface font does not have ----------------------
//
// Outfit covers Latin. A library of anime has file names in Japanese (and
// sometimes Korean), which would be drawn as empty boxes. Windows, macOS and
// most Linux desktops already ship fonts for them, so those are borrowed, and
// only when a scan actually finds such names: the Korean font of macOS alone
// is 55 MB, too much to read at every start.

static BASE_FONTS: Mutex<Option<FontDefinitions>> = Mutex::new(None);
/// A font for each script is looked for once, found or not.
static CJK_TRIED: AtomicBool = AtomicBool::new(false);
static HANGUL_TRIED: AtomicBool = AtomicBool::new(false);

const CJK_FONTS: &[&str] = &[
    "/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc",
    "/System/Library/Fonts/Hiragino Sans GB.ttc",
    "/System/Library/Fonts/Supplemental/Arial Unicode.ttf",
    r"C:\Windows\Fonts\YuGothM.ttc",
    r"C:\Windows\Fonts\msyh.ttc",
    r"C:\Windows\Fonts\meiryo.ttc",
    r"C:\Windows\Fonts\simsun.ttc",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
];

const HANGUL_FONTS: &[&str] = &[
    "/System/Library/Fonts/AppleSDGothicNeo.ttc",
    r"C:\Windows\Fonts\malgun.ttf",
    "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
    "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
];

/// Whether this machine has a font for Japanese and Chinese names at all. The
/// test harness panics on a glyph nothing can draw, so a test that draws such
/// names has to know before it builds the window.
#[cfg(test)]
pub fn cjk_font_available() -> bool {
    first_font(CJK_FONTS).is_some()
}

/// `(needs a CJK font, needs a Hangul font)` for a piece of text.
pub fn scripts_needed(text: &str) -> (bool, bool) {
    let (mut cjk, mut hangul) = (false, false);
    for character in text.chars() {
        match character as u32 {
            0x3000..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xFF00..=0xFFEF => cjk = true,
            0x1100..=0x11FF | 0x3130..=0x318F | 0xAC00..=0xD7AF => hangul = true,
            _ => {}
        }
        if cjk && hangul {
            break;
        }
    }
    (cjk, hangul)
}

fn first_font(candidates: &[&str]) -> Option<(String, Vec<u8>)> {
    candidates.iter().find_map(|path| {
        std::fs::read(path)
            .ok()
            .map(|bytes| ((*path).to_string(), bytes))
    })
}

/// Load, once, a system font for each script found in `texts`.
pub fn ensure_fallback_fonts<'a>(
    ctx: &eframe::egui::Context,
    texts: impl IntoIterator<Item = &'a str>,
) {
    let (mut cjk, mut hangul) = (false, false);
    for text in texts {
        let (c, h) = scripts_needed(text);
        cjk |= c;
        hangul |= h;
        if cjk && hangul {
            break;
        }
    }
    let want_cjk = cjk && !CJK_TRIED.load(Ordering::Relaxed);
    let want_hangul = hangul && !HANGUL_TRIED.load(Ordering::Relaxed);
    if !want_cjk && !want_hangul {
        return;
    }
    let mut base = BASE_FONTS.lock().unwrap_or_else(|error| error.into_inner());
    let Some(fonts) = base.as_mut() else {
        return;
    };
    let mut changed = false;
    for (wanted, flag, candidates, name) in [
        (want_cjk, &CJK_TRIED, CJK_FONTS, "system-cjk"),
        (want_hangul, &HANGUL_TRIED, HANGUL_FONTS, "system-hangul"),
    ] {
        if !wanted {
            continue;
        }
        // Found or not, do not look again.
        flag.store(true, Ordering::Relaxed);
        if let Some((_, bytes)) = first_font(candidates) {
            fonts
                .font_data
                .insert(name.to_string(), Arc::new(FontData::from_owned(bytes)));
            for family in [FontFamily::Proportional, FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .push(name.to_string());
            }
            changed = true;
        }
    }
    if changed {
        ctx.set_fonts(fonts.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scripts_are_detected_in_file_names() {
        assert_eq!(scripts_needed("Frieren - S01E01.mkv"), (false, false));
        assert_eq!(scripts_needed("Café déjà vu"), (false, false));
        assert_eq!(scripts_needed("進撃の巨人"), (true, false));
        assert_eq!(scripts_needed("ひらがな カタカナ"), (true, false));
        assert_eq!(scripts_needed("기생충"), (false, true));
        assert_eq!(scripts_needed("進撃 기생충"), (true, true));
        assert_eq!(
            scripts_needed("Ｆｕｌｌ"),
            (true, false),
            "full-width Latin needs a CJK font"
        );
    }
}
