// A graphical app: without this Windows opens a console window behind it.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod dialogs;
mod icon;
mod markdown;
mod menu;
#[cfg(target_os = "macos")]
mod native_menu;
mod platform;
mod settings;
mod table;
mod theme;
mod worker;

fn main() -> eframe::Result<()> {
    icon::install();

    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 820.0])
            .with_min_inner_size([1050.0, 680.0])
            .with_title("Fotogramma — H.265 cinema library"),
        renderer: eframe::Renderer::Glow,
        ..Default::default()
    };

    eframe::run_native(
        "Fotogramma — H.265 cinema library",
        options,
        Box::new(|cc| Ok(Box::new(app::FotogrammaApp::new(cc)))),
    )
}
