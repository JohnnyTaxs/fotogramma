//! Cinema Pop theme, rebuilt for egui from the original Qt stylesheet.

use eframe::egui::{
    Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Stroke, TextStyle,
    Visuals,
};
use std::collections::BTreeMap;
use std::sync::Arc;

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
    visuals.window_stroke = Stroke::new(1.0, CORAL);
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
    widgets.open.bg_stroke = Stroke::new(1.0, CORAL);
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
