//! Main window state and layout, rebuilt from the Qt application.

use eframe::egui::{
    self, Align, Align2, CornerRadius, FontId, Key, Layout, RichText, Sense, Stroke, StrokeKind,
    Vec2,
};
use fotogramma_core::formatting::{format_bytes, format_int};
use fotogramma_core::media_tools::{ffprobe_path, ffprobe_version};
use fotogramma_core::scanner::{summarize, DiscoveryWarning, Summary, VideoInfo};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::dialogs::{
    self, CloseChoice, ConversionDialog, ConversionPlan, ConversionSession, DialogOutcome,
    ProgressAction,
};
use crate::platform;
use crate::settings::Settings;
use crate::table::{self, FilterCategory, SortState, TableAction};
use crate::theme;
use crate::worker::{self, ScanMessage};

#[derive(Clone, Debug, PartialEq)]
enum Page {
    Welcome,
    Results,
    Error(String),
}

struct Args {
    directory: Option<String>,
    scan_on_start: bool,
    smoke_test: bool,
}

fn parse_args() -> Args {
    let mut args = Args {
        directory: None,
        scan_on_start: false,
        smoke_test: false,
    };
    let mut iterator = std::env::args().skip(1);
    while let Some(argument) = iterator.next() {
        match argument.as_str() {
            "--directory" => args.directory = iterator.next(),
            "--scan-on-start" => args.scan_on_start = true,
            "--smoke-test" => args.smoke_test = true,
            _ => {}
        }
    }
    args
}

pub struct FotogrammaApp {
    settings: Settings,
    directory: String,
    page: Page,
    scanning: bool,
    scan_progress: Option<(usize, Option<usize>, String)>,
    scan_cancel: Option<Arc<AtomicBool>>,
    scan_receiver: Option<Receiver<ScanMessage>>,
    videos: Vec<VideoInfo>,
    summary: Summary,
    warnings: Vec<DiscoveryWarning>,
    cancelled: bool,
    filter: FilterCategory,
    query: String,
    sort: SortState,
    selected: HashSet<PathBuf>,
    selection_anchor: Option<usize>,
    status: Option<(String, Instant)>,
    engine_status: String,
    engine_ok: bool,
    folder_error: Option<String>,
    conversion_dialog: Option<ConversionDialog>,
    session: Option<ConversionSession>,
    details: Option<VideoInfo>,
    close_confirm: bool,
    pending_close: bool,
    save_on_close: bool,
    args: Args,
    scan_completed_once: bool,
    close_after_scan_at: Option<Instant>,
    started_at: Instant,
    last_size: Vec2,
}

impl FotogrammaApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let mut settings = Settings::load();
        let args = parse_args();
        if let Some(directory) = &args.directory {
            settings.directory = directory.clone();
        }
        if let (Some(width), Some(height)) = (settings.window_width, settings.window_height) {
            cc.egui_ctx
                .send_viewport_cmd(egui::ViewportCommand::InnerSize(Vec2::new(
                    width.max(1050.0),
                    height.max(680.0),
                )));
        }
        let (engine_status, engine_ok) = engine_status();
        let directory = settings.directory.clone();
        let mut app = Self {
            settings,
            directory,
            page: Page::Welcome,
            scanning: false,
            scan_progress: None,
            scan_cancel: None,
            scan_receiver: None,
            videos: Vec::new(),
            summary: Summary::default(),
            warnings: Vec::new(),
            cancelled: false,
            filter: FilterCategory::Convert,
            query: String::new(),
            sort: SortState::default(),
            selected: HashSet::new(),
            selection_anchor: None,
            status: None,
            engine_status,
            engine_ok,
            folder_error: None,
            conversion_dialog: None,
            session: None,
            details: None,
            close_confirm: false,
            pending_close: false,
            save_on_close: true,
            args,
            scan_completed_once: false,
            close_after_scan_at: None,
            started_at: Instant::now(),
            last_size: Vec2::new(1280.0, 820.0),
        };
        if app.args.scan_on_start {
            app.start_scan(&cc.egui_ctx);
        }
        app
    }

    fn is_busy(&self) -> bool {
        self.scanning
            || self
                .session
                .as_ref()
                .map(|session| session.is_running())
                .unwrap_or(false)
    }

    fn poll_scan(&mut self, ctx: &egui::Context) {
        let mut messages = Vec::new();
        if let Some(receiver) = &self.scan_receiver {
            while let Ok(message) = receiver.try_recv() {
                messages.push(message);
            }
        }
        for message in messages {
            match message {
                ScanMessage::Progress {
                    processed,
                    total,
                    current,
                } => {
                    self.scan_progress = Some((processed, total, current));
                }
                ScanMessage::Completed {
                    videos,
                    warnings,
                    cancelled,
                } => {
                    self.scanning = false;
                    self.scan_receiver = None;
                    self.scan_cancel = None;
                    self.scan_progress = None;
                    self.videos = videos;
                    self.warnings = warnings;
                    self.cancelled = cancelled;
                    self.summary = summarize(&self.videos, None);
                    self.selected.clear();
                    self.selection_anchor = None;
                    self.filter = FilterCategory::Convert;
                    self.page = Page::Results;
                    self.scan_completed_once = true;
                    if self.args.smoke_test {
                        self.close_after_scan_at = Some(Instant::now());
                    }
                    self.set_status("Nessun file viene modificato", Duration::from_secs(4));
                    ctx.request_repaint();
                }
                ScanMessage::Failed(message) => {
                    self.scanning = false;
                    self.scan_receiver = None;
                    self.scan_cancel = None;
                    self.scan_progress = None;
                    self.page = Page::Error(message);
                    ctx.request_repaint();
                }
            }
        }
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        if self.scanning {
            self.folder_error = Some("È già in corso un’analisi.".to_string());
            return;
        }
        let directory = self.directory.trim().to_string();
        if directory.is_empty() {
            self.folder_error = Some("Inserisci o scegli una cartella da analizzare.".to_string());
            return;
        }
        let path = PathBuf::from(&directory);
        if !path.is_dir() {
            self.folder_error =
                Some("La cartella indicata non esiste o non è accessibile.".to_string());
            return;
        }
        self.folder_error = None;
        self.settings.directory = directory.clone();
        self.settings.save();
        self.scanning = true;
        self.scan_progress = Some((0, None, String::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        self.scan_cancel = Some(cancel.clone());
        self.scan_receiver = Some(worker::spawn_scan(path, cancel, ctx.clone()));
        ctx.request_repaint();
    }

    fn cancel_scan(&mut self) {
        if let Some(cancel) = &self.scan_cancel {
            cancel.store(true, Ordering::Relaxed);
        }
    }

    fn set_status(&mut self, message: impl Into<String>, duration: Duration) {
        self.status = Some((message.into(), Instant::now() + duration));
    }

    fn selected_videos(&self) -> Vec<VideoInfo> {
        self.videos
            .iter()
            .filter(|video| video.category == fotogramma_core::scanner::Category::Convert)
            .filter(|video| self.selected.contains(&video.path))
            .cloned()
            .collect()
    }

    fn update_actions_for_click(&mut self, click: table::RowClick, visible: &[usize]) {
        let path = self.videos[click.index].path.clone();
        let additive = click.modifiers.command || click.modifiers.ctrl;
        if click.modifiers.shift {
            if let Some(anchor) = self.selection_anchor {
                let start = visible.iter().position(|index| *index == anchor);
                let end = visible.iter().position(|index| *index == click.index);
                if let (Some(start), Some(end)) = (start, end) {
                    let (start, end) = if start <= end {
                        (start, end)
                    } else {
                        (end, start)
                    };
                    for index in &visible[start..=end] {
                        self.selected.insert(self.videos[*index].path.clone());
                    }
                }
            } else {
                self.selected.insert(path);
            }
        } else if additive {
            if !self.selected.insert(path.clone()) {
                self.selected.remove(&path);
            }
            self.selection_anchor = Some(click.index);
        } else {
            self.selected.clear();
            self.selected.insert(path);
            self.selection_anchor = Some(click.index);
        }
    }

    fn show_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(dialog) = self.conversion_dialog.as_mut() {
            match dialog.show(ctx) {
                DialogOutcome::Start(plan) => {
                    self.conversion_dialog = None;
                    self.start_conversion(plan, ctx);
                }
                DialogOutcome::Cancel => {
                    self.conversion_dialog = None;
                }
                DialogOutcome::None => {}
            }
        }

        let mut close_session = false;
        if let Some(session) = self.session.as_mut() {
            match dialogs::show_progress(ctx, session) {
                ProgressAction::Close => close_session = true,
                ProgressAction::OpenFolder => {
                    let destination = session.destination_dir().to_path_buf();
                    platform::open_folder(&destination);
                }
                ProgressAction::Cancel | ProgressAction::None => {}
            }
        }
        if close_session {
            self.mark_replaced_videos();
            self.session = None;
        }

        if let Some(video) = self.details.clone() {
            if dialogs::show_details(ctx, &video) {
                self.details = None;
            }
        }

        if self.close_confirm {
            match dialogs::show_close_confirm(ctx) {
                Some(CloseChoice::StopAndClose) => {
                    self.close_confirm = false;
                    self.pending_close = true;
                    self.cancel_scan();
                    if let Some(session) = &self.session {
                        session.cancel();
                    }
                    if !self.is_busy() {
                        self.pending_close = false;
                        self.save(ctx);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                Some(CloseChoice::KeepOpen) => {
                    self.close_confirm = false;
                }
                None => {}
            }
        }
    }

    fn start_conversion(&mut self, plan: ConversionPlan, ctx: &egui::Context) {
        let cancel = Arc::new(AtomicBool::new(false));
        let receiver = worker::spawn_conversion(
            plan.videos.clone(),
            plan.profile.clone(),
            plan.destination_dir.clone(),
            plan.container.clone(),
            plan.replace_original,
            cancel.clone(),
            ctx.clone(),
        );
        self.session = Some(ConversionSession::new(plan, receiver, cancel));
        ctx.request_repaint();
    }

    fn mark_replaced_videos(&mut self) {
        if let Some(session) = &self.session {
            let replaced: Vec<PathBuf> = session
                .results
                .iter()
                .filter(|result| result.replaced)
                .map(|result| result.source.clone())
                .collect();
            for video in &mut self.videos {
                if replaced.contains(&video.path) {
                    video.category = fotogramma_core::scanner::Category::Compatible;
                    video.codec = Some("hevc".to_string());
                    video.error = None;
                }
            }
            if !replaced.is_empty() {
                self.summary = summarize(&self.videos, None);
            }
        }
    }

    fn save(&mut self, ctx: &egui::Context) {
        if !self.save_on_close {
            return;
        }
        self.settings.window_width = Some(self.last_size.x);
        self.settings.window_height = Some(self.last_size.y);
        self.settings.save();
        let _ = ctx;
    }

    fn header(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Fotogramma").size(theme::BRAND_SIZE).strong());
                ui.label(
                    RichText::new("H.265 cinema library")
                        .small()
                        .color(theme::MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let color = if self.engine_ok {
                    theme::MINT
                } else {
                    theme::ERROR
                };
                ui.label(
                    RichText::new(&self.engine_status)
                        .font(theme::mono(10.0))
                        .color(color),
                );
                ui.add_space(16.0);
                ui.label(
                    RichText::new("Solo sul PC · Solo lettura")
                        .small()
                        .color(theme::MUTED),
                );
            });
        });
    }

    fn control_deck(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        theme::card_frame().show(ui, |ui| {
            ui.label(RichText::new("Cartella della libreria").strong());
            ui.label(
                RichText::new("Analisi ricorsiva di MP4, MKV e WebM. Nessuna modifica ai file.")
                    .small()
                    .color(theme::MUTED),
            );
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                let button_width = 118.0 + 150.0 + 20.0;
                let width = (ui.available_width() - button_width).max(200.0);
                let edit = ui.add_sized(
                    [width, 34.0],
                    egui::TextEdit::singleline(&mut self.directory)
                        .font(theme::mono(11.0))
                        .hint_text("Es. /Filmati oppure D:\\Video"),
                );
                if edit.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter)) {
                    self.start_scan(ctx);
                }
                if ui
                    .add_enabled(!self.scanning, theme::secondary_button("Sfoglia"))
                    .clicked()
                {
                    if let Some(selected) = rfd::FileDialog::new()
                        .set_directory(if self.directory.trim().is_empty() {
                            home_directory()
                        } else {
                            PathBuf::from(self.directory.trim())
                        })
                        .pick_folder()
                    {
                        self.directory = selected.to_string_lossy().into_owned();
                        self.folder_error = None;
                    }
                }
                if ui
                    .add_enabled(!self.scanning, theme::primary_button("Analizza libreria"))
                    .clicked()
                {
                    self.start_scan(ctx);
                }
            });

            if self.scanning {
                ui.add_space(4.0);
                let (processed, total, current) =
                    self.scan_progress
                        .clone()
                        .unwrap_or((0, None, String::new()));
                // The stop button keeps the right edge; the bar and the counter
                // share what is left, and the counter truncates instead of
                // pushing the row past the window.
                let cancelled = ui
                    .horizontal(|ui| {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let cancelled = ui.add(theme::secondary_button("Interrompi")).clicked();
                            ui.with_layout(
                                Layout::left_to_right(Align::Center),
                                |ui| match total {
                                    Some(total) => {
                                        let fraction = if total == 0 {
                                            0.0
                                        } else {
                                            processed as f32 / total as f32
                                        };
                                        let counter = format!(
                                            "{} / {} · {current}",
                                            format_int(processed),
                                            format_int(total)
                                        );
                                        let width = counter_width(ui, &counter);
                                        let bar_width = (ui.available_width() - width).max(60.0);
                                        ui.add(
                                            egui::ProgressBar::new(fraction)
                                                .desired_height(8.0)
                                                .desired_width(bar_width)
                                                .fill(theme::CORAL),
                                        );
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(counter).small().color(theme::MUTED),
                                            )
                                            .truncate(),
                                        );
                                    }
                                    None => {
                                        ui.add(egui::Spinner::new());
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(format!(
                                                    "{} video trovati · {current}",
                                                    format_int(processed)
                                                ))
                                                .small()
                                                .color(theme::MUTED),
                                            )
                                            .truncate(),
                                        );
                                    }
                                },
                            );
                            cancelled
                        })
                    })
                    .inner;
                if cancelled.inner {
                    self.cancel_scan();
                }
            }

            if let Some(error) = &self.folder_error {
                ui.label(RichText::new(error).color(theme::ERROR));
            }
        });
    }

    fn welcome_page(&self, ui: &mut egui::Ui) {
        theme::card_frame().show(ui, |ui| {
            ui.set_min_height(ui.available_height().max(320.0));
            ui.horizontal_top(|ui| {
                let left_width = ui.available_width() * 0.62;
                ui.allocate_ui_with_layout(
                    Vec2::new(left_width, ui.available_height()),
                    Layout::top_down(Align::LEFT),
                    |ui| {
                        ui.label(
                            RichText::new("Pronta per H.265.")
                                .size(theme::HERO_SIZE)
                                .strong(),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(
                                "Scegli una cartella e avvia l’analisi. Fotogramma separa i video già pronti \
                                 da quelli che potresti convertire, senza spostare o modificare nulla.",
                            )
                            .color(theme::MUTED),
                        );
                        ui.add_space(18.0);
                        for (number, heading, description) in [
                            ("01", "Scansione", "Lettura del primo flusso video tramite ffprobe."),
                            ("02", "Selezione", "Filtri, ricerca e ordinamento per isolare i candidati."),
                            ("03", "Esportazione", "Un file di testo con i soli percorsi da convertire."),
                        ] {
                            ui.label(
                                RichText::new(format!("{number}  {heading}"))
                                    .strong()
                                    .color(theme::TEXT),
                            );
                            ui.label(
                                RichText::new(description)
                                    .small()
                                    .color(theme::MUTED),
                            );
                            ui.add_space(6.0);
                        }
                    },
                );
                ui.add_space(24.0);
                ui.vertical(|ui| {
                    ui.set_width(ui.available_width());
                    ui.add_space(24.0);
                    for (text, compatible) in [
                        ("H.265 · HEVC", true),
                        ("AV1 · AOMedia", true),
                        ("H.264 · AVC", false),
                        ("VP9 · WebM", false),
                    ] {
                        chip(ui, text, compatible);
                        ui.add_space(8.0);
                    }
                });
            });
        });
    }

    fn results_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let total = self.summary.total;
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("La tua libreria")
                        .size(theme::PAGE_TITLE_SIZE)
                        .strong(),
                );
                ui.label(
                    RichText::new(&self.settings.directory)
                        .font(theme::mono(10.0))
                        .color(theme::MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(theme::primary_button("Esporta da convertire"))
                    .clicked()
                {
                    self.export_videos();
                }
            });
        });
        ui.add_space(6.0);

        let spacing = 10.0;
        let width = ((ui.available_width() - spacing * 3.0) / 5.0).max(100.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            let cards = [
                (
                    "all",
                    self.summary.total,
                    "Video presenti",
                    format!(
                        "{} analizzati",
                        format_bytes(Some(self.summary.total_bytes))
                    ),
                    false,
                    width,
                ),
                (
                    "convert",
                    self.summary.to_convert,
                    "Da convertire",
                    format!(
                        "{} da elaborare",
                        format_bytes(Some(self.summary.to_convert_bytes))
                    ),
                    true,
                    width * 2.0,
                ),
                (
                    "compatible",
                    self.summary.compatible,
                    "Già compatibili",
                    "H.265 oppure AV1".to_string(),
                    false,
                    width,
                ),
                (
                    "error",
                    self.summary.errors,
                    "Non leggibili",
                    "Da controllare".to_string(),
                    false,
                    width,
                ),
            ];
            for (key, value, label, detail, accent, card_width) in cards {
                let filter = match key {
                    "convert" => FilterCategory::Convert,
                    "compatible" => FilterCategory::Compatible,
                    "error" => FilterCategory::Error,
                    _ => FilterCategory::All,
                };
                let selected = self.filter == filter;
                if stat_card(
                    ui, card_width, 66.0, value, label, &detail, accent, selected,
                )
                .clicked()
                {
                    self.filter = filter;
                }
            }
        });
        ui.add_space(10.0);

        let caption = if total > 0 {
            format!(
                "{}% da convertire · {}% compatibili",
                (self.summary.to_convert as f32 / total as f32 * 100.0).round() as u32,
                (self.summary.compatible as f32 / total as f32 * 100.0).round() as u32
            )
        } else {
            "Nessun video trovato".to_string()
        };
        ui.horizontal(|ui| {
            let caption_width = 220.0;
            let bar_width = (ui.available_width() - caption_width - 8.0).max(160.0);
            composition_bar(
                ui,
                total,
                self.summary.to_convert,
                self.summary.compatible,
                self.summary.errors,
                bar_width,
            );
            ui.allocate_ui_with_layout(
                Vec2::new(caption_width, 14.0),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(caption).small().color(theme::MUTED))
                            .truncate(),
                    );
                },
            );
        });

        if self.cancelled || !self.warnings.is_empty() {
            let mut messages = Vec::new();
            if self.cancelled {
                messages.push("Analisi interrotta: i risultati sono parziali.");
            }
            if !self.warnings.is_empty() {
                messages.push("cartelle non accessibili: il riepilogo può essere incompleto.");
            }
            let text = if self.warnings.is_empty() {
                messages.join(" ")
            } else {
                format!(
                    "{} {} {}",
                    messages[0],
                    self.warnings.len(),
                    messages.last().unwrap()
                )
            };
            let response = ui.label(RichText::new(text).color(theme::CORAL_HOVER));
            let tooltip = self
                .warnings
                .iter()
                .map(|warning| format!("{}: {}", warning.path.display(), warning.message))
                .collect::<Vec<_>>()
                .join("\n");
            if !tooltip.is_empty() {
                response.on_hover_text(tooltip);
            }
        }

        let insight = if self.summary.to_convert > 0 {
            format!(
                "{} file richiedono un’azione. Selezionali nella tabella per cercare online o convertire direttamente.",
                format_int(self.summary.to_convert)
            )
        } else {
            "Tutti i file sono già compatibili con H.265 o AV1: non serve alcuna conversione."
                .to_string()
        };
        ui.label(RichText::new(insight).small().color(theme::MUTED));

        ui.horizontal(|ui| {
            for (filter, label) in [
                (FilterCategory::All, "Tutti"),
                (FilterCategory::Convert, "Da convertire"),
                (FilterCategory::Compatible, "Compatibili"),
                (FilterCategory::Error, "Errori"),
            ] {
                if filter_pill(ui, label, self.filter == filter).clicked() {
                    self.filter = filter;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.add_sized(
                    [340.0, 34.0],
                    egui::TextEdit::singleline(&mut self.query).hint_text("Cerca nella libreria…"),
                );
            });
        });

        let selected = self.selected_videos();
        ui.horizontal(|ui| {
            let label = if selected.is_empty() {
                "Seleziona uno o più video da convertire per vedere le azioni".to_string()
            } else {
                format!(
                    "{} video da convertire selezionati",
                    format_int(selected.len())
                )
            };
            ui.label(RichText::new(label).small().color(theme::MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !selected.is_empty(),
                        theme::primary_button("Converti selezionati"),
                    )
                    .on_hover_text("Crea nuovi file H.265 senza modificare gli originali")
                    .clicked()
                {
                    self.conversion_dialog = Some(ConversionDialog::new(selected.clone()));
                }
            });
        });

        let visible = table::visible_indices(&self.videos, self.filter, &self.query, self.sort);
        let mut action = None;
        ui.scope(|ui| {
            let height = (ui.available_height() - 26.0).max(120.0);
            ui.allocate_ui(Vec2::new(ui.available_width(), height), |ui| {
                action = table::show(ui, &self.videos, &visible, &self.selected, &mut self.sort);
            });
        });
        match action {
            Some(TableAction::Click(click)) => {
                self.update_actions_for_click(click, &visible);
            }
            Some(TableAction::Open(index)) => {
                self.details = Some(self.videos[index].clone());
            }
            None => {}
        }
        ui.label(
            RichText::new(format!(
                "{} video in questa vista",
                format_int(visible.len())
            ))
            .small()
            .color(theme::MUTED),
        );
        let _ = ctx;
    }

    fn error_page(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, message: &str) {
        ui.vertical_centered(|ui| {
            ui.add_space(60.0);
            ui.label(
                RichText::new("L’analisi si è fermata")
                    .size(theme::PAGE_TITLE_SIZE)
                    .strong(),
            );
            ui.label(
                RichText::new(format!(
                    "{message}\n\nControlla la cartella e che ffprobe sia disponibile, poi riprova."
                ))
                .color(theme::MUTED),
            );
            ui.add_space(8.0);
            if ui.add(theme::primary_button("Riprova")).clicked() {
                self.start_scan(ctx);
            }
        });
    }

    fn export_videos(&mut self) {
        let convertible: Vec<String> = self
            .videos
            .iter()
            .filter(|video| video.category == fotogramma_core::scanner::Category::Convert)
            .map(|video| video.path.to_string_lossy().into_owned())
            .collect();
        if convertible.is_empty() {
            self.set_status("Nessun video da convertire", Duration::from_secs(4));
            return;
        }
        let default_file = home_directory().join("fotogramma-da-convertire.txt");
        let Some(target) = rfd::FileDialog::new()
            .set_file_name("fotogramma-da-convertire.txt")
            .set_directory(home_directory())
            .add_filter("File di testo", &["txt"])
            .save_file()
        else {
            return;
        };
        let _ = default_file;
        match std::fs::write(&target, format!("{}\n", convertible.join("\n"))) {
            Ok(()) => self.set_status(
                format!(
                    "Esportati {} percorsi in {}",
                    format_int(convertible.len()),
                    target.display()
                ),
                Duration::from_secs(7),
            ),
            Err(error) => self.set_status(
                format!("Esportazione non riuscita: {error}"),
                Duration::from_secs(7),
            ),
        }
    }

    fn smoke_tick(&mut self, ctx: &egui::Context) {
        if !self.args.smoke_test {
            return;
        }
        if self.args.scan_on_start {
            if let Some(at) = self.close_after_scan_at {
                if at.elapsed() > Duration::from_millis(300) {
                    self.save(ctx);
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            } else if self.started_at.elapsed() > Duration::from_secs(30) {
                self.save(ctx);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        } else if self.started_at.elapsed() > Duration::from_secs(2) {
            self.save(ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

impl eframe::App for FotogrammaApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.last_size = ctx
            .input(|input| input.viewport().inner_rect.map(|rect| rect.size()))
            .unwrap_or(self.last_size);
        self.poll_scan(&ctx);
        if let Some(session) = self.session.as_mut() {
            session.poll();
        }
        self.mark_replaced_videos();

        if ctx.input(|input| input.viewport().close_requested()) {
            if self.is_busy() && !self.pending_close {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.close_confirm = true;
            } else {
                self.save(&ctx);
            }
        }
        if self.pending_close && !self.is_busy() {
            self.pending_close = false;
            self.save(&ctx);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        egui::Panel::bottom("status-bar")
            .show_separator_line(false)
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG)
                    .inner_margin(egui::Margin::symmetric(24, 6)),
            )
            .show(ui, |ui| {
                if let Some((message, until)) = &self.status {
                    if Instant::now() < *until {
                        ui.label(RichText::new(message).small().color(theme::MUTED));
                    }
                }
            });

        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(theme::BG)
                    .inner_margin(egui::Margin {
                        left: 24,
                        right: 24,
                        top: 18,
                        bottom: 8,
                    }),
            )
            .show(ui, |ui| {
                self.header(ui);
                ui.add_space(10.0);
                self.control_deck(ui, &ctx);
                ui.add_space(14.0);
                match self.page.clone() {
                    Page::Welcome => self.welcome_page(ui),
                    Page::Results => self.results_page(ui, &ctx),
                    Page::Error(message) => self.error_page(ui, &ctx, &message),
                }
            });

        self.show_dialogs(&ctx);
        if self.is_busy() || self.conversion_dialog.is_some() {
            ctx.request_repaint_after(Duration::from_millis(80));
        }
        if self.args.smoke_test {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        self.smoke_tick(&ctx);
    }
}

fn engine_status() -> (String, bool) {
    match ffprobe_path() {
        Ok(path) => match ffprobe_version(&path) {
            Some(version) => {
                let parts: Vec<&str> = version.split_whitespace().collect();
                if parts.len() > 2 {
                    (format!("ffprobe {}", parts[2]), true)
                } else {
                    ("ffprobe non disponibile".to_string(), false)
                }
            }
            None => ("ffprobe non disponibile".to_string(), false),
        },
        Err(_) => ("ffprobe mancante".to_string(), false),
    }
}

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Width of the scan counter, so the progress bar can reserve room for it.
fn counter_width(ui: &egui::Ui, text: &str) -> f32 {
    let font = egui::TextStyle::Small.resolve(ui.style());
    ui.painter()
        .layout_no_wrap(text.to_string(), font, theme::MUTED)
        .size()
        .x
        + 10.0
}

fn chip(ui: &mut egui::Ui, text: &str, compatible: bool) {
    let (rect, _response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 66.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(12), theme::BG_DEEP);
    painter.rect_stroke(
        rect,
        CornerRadius::same(12),
        Stroke::new(1.0, theme::LINE),
        StrokeKind::Inside,
    );
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::monospace(18.0),
        if compatible {
            theme::MINT
        } else {
            theme::CORAL
        },
    );
}

fn filter_pill(ui: &mut egui::Ui, label: &str, selected: bool) -> egui::Response {
    let text_color = if selected {
        theme::CORAL_INK
    } else {
        theme::LAVENDER
    };
    ui.add(
        egui::Button::new(RichText::new(label).color(text_color))
            .fill(if selected {
                theme::CORAL
            } else {
                theme::RAISED
            })
            .stroke(Stroke::new(1.0, theme::INPUT_BORDER))
            .corner_radius(CornerRadius::same(16)),
    )
}

#[allow(clippy::too_many_arguments)]
fn stat_card(
    ui: &mut egui::Ui,
    width: f32,
    height: f32,
    value: usize,
    label: &str,
    detail: &str,
    accent: bool,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    let fill = if accent { theme::CORAL } else { theme::SURFACE };
    let stroke = if selected {
        Stroke::new(
            2.0,
            if accent {
                theme::CORAL_INK
            } else {
                theme::CORAL_HOVER
            },
        )
    } else {
        Stroke::new(1.0, theme::LINE)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(12), fill);
    painter.rect_stroke(rect, CornerRadius::same(12), stroke, StrokeKind::Inside);
    let ink = if accent {
        theme::CORAL_INK
    } else {
        theme::TEXT
    };
    let muted = if accent {
        theme::CORAL_INK.gamma_multiply(0.8)
    } else {
        theme::MUTED
    };
    painter.text(
        rect.left_top() + Vec2::new(16.0, 13.0),
        Align2::LEFT_TOP,
        format!("{} · {}", format_int(value), label),
        FontId::proportional(if accent { 17.0 } else { 15.0 }),
        ink,
    );
    painter.text(
        rect.left_bottom() + Vec2::new(16.0, -12.0),
        Align2::LEFT_BOTTOM,
        detail,
        FontId::proportional(11.0),
        muted,
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn composition_bar(
    ui: &mut egui::Ui,
    total: usize,
    convert: usize,
    compatible: usize,
    errors: usize,
    width: f32,
) {
    let (rect, _response) = ui.allocate_exact_size(Vec2::new(width, 10.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(4), theme::TRACK);
    if total == 0 {
        return;
    }
    let mut x = rect.left();
    for (count, color) in [
        (convert, theme::CORAL),
        (compatible, theme::MINT),
        (errors, theme::ERROR),
    ] {
        let width = rect.width() * count as f32 / total as f32;
        if width > 0.0 {
            let segment = egui::Rect::from_min_size(
                egui::pos2(x, rect.top()),
                Vec2::new(width, rect.height()),
            );
            painter.rect_filled(segment, CornerRadius::same(2), color);
            x += width;
        }
    }
}

#[cfg(test)]
mod snapshot_tests {
    use super::*;
    use fotogramma_core::scanner::Category;

    fn demo_video(
        name: &str,
        codec: &str,
        category: Category,
        size: u64,
        duration: f64,
        error: Option<&str>,
    ) -> VideoInfo {
        VideoInfo {
            path: PathBuf::from(format!("/Filmati/{name}")),
            relative_path: name.to_string(),
            name: name.to_string(),
            folder: "Vacanze".to_string(),
            extension: name.rsplit('.').next().unwrap_or("mkv").to_string(),
            category,
            codec: Some(codec.to_string()),
            size: Some(size),
            duration: Some(duration),
            width: Some(1920),
            height: Some(1080),
            error: error.map(|value| value.to_string()),
        }
    }

    pub(super) fn demo_app(cc: &mut eframe::CreationContext<'_>) -> FotogrammaApp {
        let mut app = FotogrammaApp::new(cc);
        app.directory = "/Filmati".to_string();
        app.settings.directory = "/Filmati".to_string();
        app.videos = vec![
            demo_video(
                "Tramonto.mkv",
                "h264",
                Category::Convert,
                587_282,
                12.0,
                None,
            ),
            demo_video(
                "Gita al lago.mp4",
                "h264",
                Category::Convert,
                1_036_781,
                8.0,
                None,
            ),
            demo_video(
                "Film HEVC.mp4",
                "hevc",
                Category::Compatible,
                250_000,
                15.0,
                None,
            ),
            demo_video(
                "Animazione AV1.webm",
                "av1",
                Category::Compatible,
                120_000,
                6.0,
                None,
            ),
            demo_video(
                "Vecchio video rovinato.mkv",
                "h264",
                Category::Error,
                40_000,
                0.0,
                Some("Invalid data found when processing input"),
            ),
        ];
        app.summary = summarize(&app.videos, None);
        app.page = Page::Results;
        app.filter = FilterCategory::All;
        app.scan_completed_once = true;
        app
    }

    #[test]
    fn main_window_renders() {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1280.0, 820.0))
            .build_eframe(demo_app);
        harness.run();
        harness.snapshot("main-window");
    }

    #[test]
    fn the_scan_row_stays_inside_the_window() {
        use egui_kittest::kittest::Queryable;

        for width in [1050.0, 1280.0, 2024.0] {
            let mut harness = egui_kittest::Harness::builder()
                .with_size(egui::vec2(width, 820.0))
                .build_eframe(|cc| {
                    let mut app = demo_app(cc);
                    app.page = Page::Welcome;
                    app.scanning = true;
                    app.scan_progress = Some((96, Some(100), "Vacanze/Tramonto.mkv".to_string()));
                    app
                });
            harness.run();
            let stop = harness.get_by_label("Interrompi").rect();
            assert!(
                stop.right() <= width,
                "the stop button ends at {} in a {width} wide window",
                stop.right()
            );
            assert!(
                stop.left() > width / 2.0,
                "the stop button is pinned to the right edge"
            );
        }
    }

    #[test]
    fn conversion_dialog_renders() {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1280.0, 820.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.selected.insert(PathBuf::from("/Filmati/Tramonto.mkv"));
                app.selected
                    .insert(PathBuf::from("/Filmati/Gita al lago.mp4"));
                let selected = app.selected_videos();
                app.conversion_dialog = Some(ConversionDialog::new(selected));
                app
            });
        harness.run();
        harness.snapshot("conversion-dialog");
    }
}

#[cfg(test)]
mod small_snapshot_tests {
    use super::snapshot_tests::demo_app;
    use super::*;

    #[test]
    fn main_window_small_renders() {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        harness.snapshot("main-window-small");
    }

    #[test]
    fn conversion_dialog_small_renders() {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.selected.insert(PathBuf::from("/Filmati/Tramonto.mkv"));
                app.selected
                    .insert(PathBuf::from("/Filmati/Gita al lago.mp4"));
                let selected = app.selected_videos();
                app.conversion_dialog = Some(ConversionDialog::new(selected));
                app
            });
        harness.run();
        harness.snapshot("conversion-dialog-small");
    }
}
