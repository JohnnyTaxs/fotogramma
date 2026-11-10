//! A small easter egg: a chibi girl who runs along the progress bar towards a
//! TV that is switched off. She never gets there until the conversion has
//! really finished: then she reaches it and the TV turns on.
//!
//! The frames are in `deployment/mascot.png` (6 of a run cycle, 2 of cheering)
//! and `deployment/tv.png` (off, on), drawn in code by
//! `deployment/make_mascot.py`, so the art belongs to the project. She is
//! switched off in Settings.

use eframe::egui::{self, pos2, vec2, Color32, Rect};

const GIRL_PNG: &[u8] = include_bytes!("../../../deployment/mascot.png");
const TV_PNG: &[u8] = include_bytes!("../../../deployment/tv.png");
const RUN_FRAMES: usize = 6;
const GIRL_FRAMES: usize = 8;
/// Steps per second while she runs, and per second while she cheers.
const RUN_STEPS: f64 = 12.0;
const CHEER_STEPS: f64 = 3.0;
/// On screen she is 30 × 36 points (the frames are 40 × 48), the TV 30 × 30.
const SCALE: f32 = 0.75;
const GIRL_W: f32 = 40.0 * SCALE;
pub const GIRL_H: f32 = 48.0 * SCALE;
const TV_SIZE: f32 = 40.0 * SCALE;
/// The gap between the end of the bar and the TV: wide enough for her to stand
/// at the end of the bar (her centre on the head of the fill) without touching it.
const TV_GAP: f32 = GIRL_W * 0.6;
/// The room the TV takes at the end of the bar.
pub const TV_ROOM: f32 = TV_SIZE + TV_GAP;
/// Seconds she needs for the last dash once the conversion has finished.
const DASH_SECONDS: f64 = 0.7;

/// What the bar is doing, which decides what she does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scene {
    Running,
    Paused,
    /// Finished for good: she arrives and the TV turns on.
    Done,
    /// Stopped or failed: she stands and the TV stays off.
    Stopped,
}

pub struct Mascot {
    girl: egui::TextureHandle,
    tv: egui::TextureHandle,
}

fn load(ctx: &egui::Context, name: &str, png: &[u8]) -> Option<egui::TextureHandle> {
    let image = image::load_from_memory(png).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

/// Where the pieces are for a bar of `bar`: the TV past its right end, she on
/// top of it.
pub struct Geometry {
    pub bar: Rect,
    pub tv: Rect,
}

impl Geometry {
    pub fn new(bar: Rect) -> Self {
        let tv = Rect::from_min_size(
            pos2(bar.right() + TV_GAP, bar.bottom() + 2.0 - TV_SIZE),
            vec2(TV_SIZE, TV_SIZE),
        );
        Self { bar, tv }
    }

    /// Left edge of her sprite while the conversion runs: her centre is on the
    /// head of the bar's fill, so she is always on top of it (at the very start
    /// she is a little ahead of it, not outside the bar). At the end of the bar
    /// she is still short of the TV, which she only reaches when it is over.
    pub fn running_x(&self, fraction: f32) -> f32 {
        (self.bar.left() + fraction.clamp(0.0, 1.0) * self.bar.width() - GIRL_W * 0.5)
            .max(self.bar.left())
    }

    /// Where she ends up, right in front of the TV.
    pub fn arrival_x(&self) -> f32 {
        self.tv.left() - GIRL_W * 0.55
    }

    pub fn girl_rect(&self, x: f32) -> Rect {
        Rect::from_min_size(
            pos2(x, self.bar.bottom() + 3.0 - GIRL_H),
            vec2(GIRL_W, GIRL_H),
        )
    }
}

/// Which frame of the sheet to show.
pub fn girl_frame(time: f64, scene: Scene, dashing: bool) -> usize {
    match scene {
        Scene::Running => (time * RUN_STEPS) as usize % RUN_FRAMES,
        Scene::Done if dashing => (time * RUN_STEPS) as usize % RUN_FRAMES,
        Scene::Done => RUN_FRAMES + (time * CHEER_STEPS) as usize % (GIRL_FRAMES - RUN_FRAMES),
        Scene::Paused | Scene::Stopped => 0,
    }
}

fn ease(t: f64) -> f32 {
    let t = t.clamp(0.0, 1.0) as f32;
    t * t * (3.0 - 2.0 * t)
}

impl Mascot {
    pub fn load(ctx: &egui::Context) -> Option<Self> {
        Some(Self {
            girl: load(ctx, "mascot", GIRL_PNG)?,
            tv: load(ctx, "mascot-tv", TV_PNG)?,
        })
    }

    /// A progress bar `total_width` wide with her running along it. The TV is
    /// at the right end, inside that width.
    pub fn progress(
        &self,
        ui: &mut egui::Ui,
        total_width: f32,
        fraction: f32,
        scene: Scene,
        bar_color: Color32,
    ) {
        let (area, _) =
            ui.allocate_exact_size(vec2(total_width, GIRL_H + 2.0), egui::Sense::hover());
        let bar_height = 10.0;
        let bar = Rect::from_min_size(
            pos2(area.left(), area.bottom() - bar_height),
            vec2((total_width - TV_ROOM).max(40.0), bar_height),
        );
        // Drawn by hand: `ui.put` would move the layout cursor back to the end of
        // the bar, over the room kept for the TV.
        let round = egui::CornerRadius::same(5);
        ui.painter()
            .rect_filled(bar, round, ui.visuals().extreme_bg_color);
        let filled = Rect::from_min_size(
            bar.min,
            vec2(bar.width() * fraction.clamp(0.0, 1.0), bar.height()),
        );
        if filled.width() > 0.5 {
            ui.painter().rect_filled(filled, round, bar_color);
        }
        self.paint(ui, bar, fraction, scene);
    }

    fn paint(&self, ui: &egui::Ui, bar: Rect, fraction: f32, scene: Scene) {
        let geometry = Geometry::new(bar);
        // A fixed moment in tests, so a snapshot always shows the same frame.
        let time = if cfg!(test) {
            0.2
        } else {
            ui.input(|input| input.time)
        };
        let id = ui.next_auto_id().with("mascot-finished-at");
        let (x, dashing, tv_on) = if scene == Scene::Done {
            let since = ui.data_mut(|data| *data.get_temp_mut_or_insert_with(id, || time));
            let t = (time - since) / DASH_SECONDS;
            let from = geometry.running_x(fraction);
            let x = from + (geometry.arrival_x() - from) * ease(t);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_millis(120));
            (x, t < 1.0, t >= 1.0)
        } else {
            ui.data_mut(|data| data.remove::<f64>(id));
            (geometry.running_x(fraction), false, false)
        };

        let painter = ui.painter();
        let tv_uv = Rect::from_min_max(
            pos2(if tv_on { 0.5 } else { 0.0 }, 0.0),
            pos2(if tv_on { 1.0 } else { 0.5 }, 1.0),
        );
        painter.image(self.tv.id(), geometry.tv, tv_uv, Color32::WHITE);
        let frame = girl_frame(time, scene, dashing);
        let girl_uv = Rect::from_min_max(
            pos2(frame as f32 / GIRL_FRAMES as f32, 0.0),
            pos2((frame + 1) as f32 / GIRL_FRAMES as f32, 1.0),
        );
        painter.image(
            self.girl.id(),
            geometry.girl_rect(x),
            girl_uv,
            Color32::WHITE,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bar() -> Rect {
        Rect::from_min_size(pos2(100.0, 50.0), vec2(300.0, 10.0))
    }

    #[test]
    fn the_sheets_have_the_frames_the_code_expects() {
        let girl = image::load_from_memory(GIRL_PNG).unwrap();
        assert_eq!((girl.width(), girl.height()), (40 * GIRL_FRAMES as u32, 48));
        let tv = image::load_from_memory(TV_PNG).unwrap();
        assert_eq!((tv.width(), tv.height()), (80, 40));
    }

    #[test]
    fn while_it_runs_she_never_reaches_the_tv() {
        let g = Geometry::new(bar());
        let furthest = g.running_x(1.0);
        assert!(furthest + GIRL_W <= g.tv.left(), "even at 100% of the bar");
        assert!(furthest < g.arrival_x(), "the last dash is still to come");
        assert_eq!(g.running_x(0.0), 100.0);
        assert_eq!(g.running_x(9.0), furthest, "the fraction is clamped");
    }

    #[test]
    fn she_is_always_on_top_of_the_head_of_the_fill() {
        let g = Geometry::new(bar());
        for step in 0..=20 {
            let fraction = step as f32 / 20.0;
            let head = g.bar.left() + fraction * g.bar.width();
            let centre = g.running_x(fraction) + GIRL_W * 0.5;
            // Never behind the fill by more than a pixel, never far ahead of it.
            assert!(centre >= head - 1.0, "fraction {fraction}: she is behind");
            assert!(
                centre <= head + GIRL_W * 0.5 + 1.0,
                "fraction {fraction}: she is far ahead"
            );
        }
    }

    #[test]
    fn she_advances_with_the_percentage() {
        let g = Geometry::new(bar());
        assert!(g.running_x(0.25) < g.running_x(0.5));
        assert!(g.running_x(0.5) < g.running_x(0.75));
    }

    #[test]
    fn the_tv_is_past_the_end_of_the_bar_and_she_stands_on_it() {
        let g = Geometry::new(bar());
        assert!(g.tv.left() > g.bar.right());
        let girl = g.girl_rect(g.running_x(0.5));
        assert!((girl.bottom() - (g.bar.bottom() + 3.0)).abs() < 0.01);
    }

    #[test]
    fn she_runs_cheers_or_stands_as_the_scene_says() {
        let run: std::collections::HashSet<usize> = (0..60)
            .map(|step| girl_frame(f64::from(step) / 30.0, Scene::Running, false))
            .collect();
        assert_eq!(run.len(), RUN_FRAMES);
        let cheer: std::collections::HashSet<usize> = (0..60)
            .map(|step| girl_frame(f64::from(step) / 30.0, Scene::Done, false))
            .collect();
        assert!(cheer.iter().all(|frame| *frame >= RUN_FRAMES));
        assert_eq!(girl_frame(5.0, Scene::Paused, false), 0);
        assert_eq!(girl_frame(5.0, Scene::Stopped, false), 0);
        assert!(
            girl_frame(0.3, Scene::Done, true) < RUN_FRAMES,
            "she runs while she dashes"
        );
    }
}
