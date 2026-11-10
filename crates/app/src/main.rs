// A graphical app: without this Windows opens a console window behind it.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod dialogs;
mod icon;
mod markdown;
mod mascot;
mod menu;
#[cfg(target_os = "macos")]
mod native_menu;
mod platform;
mod settings;
mod table;
mod theme;
mod worker;

fn main() -> eframe::Result<()> {
    // The update helper on Windows is a copy of this program (no script, no
    // shell): it swaps the installed folder and exits without a window.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--apply-update") {
        std::process::exit(fotogramma_core::update::run_apply_update(&args[2..]));
    }
    fotogramma_core::update::clean_helper();
    icon::install();

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 820.0])
        .with_min_inner_size([1050.0, 680.0])
        .with_title("Fotogramma");
    if let Some(icon) = icon::window_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Fotogramma",
        options,
        Box::new(|cc| Ok(Box::new(app::FotogrammaApp::new(cc)))),
    )
}
