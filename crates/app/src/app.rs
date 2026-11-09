//! Main window state and layout, rebuilt from the Qt application.

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Key, Layout, RichText, Sense, Stroke,
    StrokeKind, Vec2,
};
use fotogramma_core::export::{self, Format};
use fotogramma_core::formatting::{format_bytes, format_int};
use fotogramma_core::media_tools::{ffprobe_path, ffprobe_version};
use fotogramma_core::scanner::{
    classify_codec, reclassify, summarize, DiscoveryWarning, Summary, VideoInfo,
};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::dialogs::{
    self, CloseChoice, ConversionDialog, ConversionPlan, ConversionSession, DialogOutcome,
    ProgressAction, UpdateAction, UpdateDialog, UpdateStage,
};
use crate::menu::{self, MenuAction, MenuState};
use crate::platform::{self, PowerAction};
use crate::settings::{Settings, KNOWN_CODECS};
use crate::table::{self, FilterCategory, SortState, TableAction};
use crate::theme;
use crate::worker::{self, CheckMessage, ScanMessage};
use fotogramma_core::i18n::{set_lang, tr_str, Lang};
use fotogramma_core::queue::PendingQueue;
use fotogramma_core::update::{self, InstallTarget};

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
    /// Videos converted in this session: original path -> the file made (the
    /// original path itself when it was replaced). Not saved.
    converted: HashMap<PathBuf, PathBuf>,
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
    /// Bottom edge of the central panel: the table takes what is left of it.
    panel_bottom: f32,
    show_settings: bool,
    show_about: bool,
    /// The answer to a manual update check, shown as a small card.
    update_notice: Option<dialogs::UpdateNotice>,
    /// The conversion runs in the background: a bar instead of a window.
    session_minimized: bool,
    /// Batches added while one runs; they start one after the other.
    waiting: Vec<ConversionPlan>,
    /// The first-run question: check for updates at startup?
    show_update_prompt: bool,
    /// Move the keyboard focus to the search field at the next frame.
    focus_search: bool,
    /// The menu bar drawn at the top of the window (Windows and Linux).
    in_window_menu: bool,
    #[cfg(target_os = "macos")]
    native_menu: Option<crate::native_menu::NativeMenu>,
    /// Kept to rebuild the native menu when the language changes.
    #[cfg(target_os = "macos")]
    egui_ctx: egui::Context,
    /// A queue left over from a run that did not finish.
    resume_offer: Option<PendingQueue>,
    /// Sleep or shut down when this instant arrives, unless cancelled.
    power_countdown: Option<(PowerAction, Instant)>,
    update_check: Option<Receiver<CheckMessage>>,
    update_manual: bool,
    update: Option<UpdateDialog>,
    install_target: Option<InstallTarget>,
    logo: Option<egui::TextureHandle>,
}

impl FotogrammaApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::install(&cc.egui_ctx);
        let mut settings = Settings::load();
        set_lang(Lang::resolve(
            &settings.language,
            system_locale().as_deref(),
        ));
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
            converted: HashMap::new(),
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
            panel_bottom: 0.0,
            show_settings: false,
            show_about: false,
            update_notice: None,
            session_minimized: false,
            waiting: Vec::new(),
            show_update_prompt: false,
            focus_search: false,
            in_window_menu: !cfg!(target_os = "macos") && !cfg!(test),
            #[cfg(target_os = "macos")]
            native_menu: if cfg!(test) {
                None
            } else {
                Some(crate::native_menu::NativeMenu::install(&cc.egui_ctx))
            },
            #[cfg(target_os = "macos")]
            egui_ctx: cc.egui_ctx.clone(),
            resume_offer: crate::settings::queue_file()
                .and_then(|file| PendingQueue::load(&file))
                .map(PendingQueue::still_present)
                .filter(|queue| !queue.videos.is_empty()),
            power_countdown: None,
            update_check: None,
            update_manual: false,
            update: None,
            install_target: if cfg!(test) {
                None
            } else {
                InstallTarget::detect()
            },
            logo: load_logo(&cc.egui_ctx),
        };
        if let Some(target) = &app.install_target {
            // An update interrupted earlier may have left its staging folder.
            update::clean_staging(target);
        }
        if app.settings.check_updates && updates_allowed(&app.args) {
            app.start_update_check(&cc.egui_ctx, false);
        } else if !app.settings.check_updates
            && !app.settings.update_choice_made
            && updates_allowed(&app.args)
        {
            // Nothing goes over the network until the user says yes.
            app.show_update_prompt = true;
        }
        if app.args.scan_on_start {
            app.start_scan(&cc.egui_ctx);
        }
        app
    }

    fn start_update_check(&mut self, ctx: &egui::Context, manual: bool) {
        if self.update_check.is_some() {
            return;
        }
        self.update_manual = manual;
        if manual {
            self.update_notice = Some(dialogs::UpdateNotice::Checking);
        }
        self.update_check = Some(worker::spawn_update_check(
            env!("CARGO_PKG_VERSION"),
            ctx.clone(),
        ));
    }

    fn poll_update(&mut self) {
        let Some(receiver) = &self.update_check else {
            return;
        };
        let Ok(message) = receiver.try_recv() else {
            return;
        };
        self.update_check = None;
        let manual = self.update_manual;
        match message {
            CheckMessage::Available(release) => {
                self.update_notice = None;
                let skipped = self.settings.skipped_update == release.tag;
                if (manual || !skipped) && self.update.is_none() {
                    self.update = Some(UpdateDialog::new(*release, self.install_target.clone()));
                }
            }
            // The card is still there only if the user did not dismiss it.
            CheckMessage::UpToDate if manual && self.update_notice.is_some() => {
                self.update_notice = Some(dialogs::UpdateNotice::UpToDate);
            }
            CheckMessage::Failed(error) if manual && self.update_notice.is_some() => {
                self.update_notice = Some(dialogs::UpdateNotice::Failed(error));
            }
            _ => {}
        }
    }

    fn show_update_dialog(&mut self, ctx: &egui::Context) {
        let busy = self.is_busy();
        let Some(mut dialog) = self.update.take() else {
            return;
        };
        dialog.poll();
        let mut keep = true;
        match dialogs::show_update(ctx, &mut dialog, busy) {
            UpdateAction::None => {}
            UpdateAction::Install => {
                if let Some(target) = dialog.target.clone() {
                    dialog.cancel = Arc::new(AtomicBool::new(false));
                    dialog.stage = UpdateStage::Downloading { done: 0, total: 0 };
                    dialog.receiver = Some(worker::spawn_update_install(
                        dialog.release.clone(),
                        target,
                        dialog.cancel.clone(),
                        ctx.clone(),
                    ));
                }
            }
            UpdateAction::Cancel => {
                dialog.cancel.store(true, Ordering::Relaxed);
                dialog.receiver = None;
                dialog.stage = UpdateStage::Available;
            }
            UpdateAction::Restart => {
                if let (Some(target), UpdateStage::Ready(staged)) = (&dialog.target, &dialog.stage)
                {
                    match update::spawn_swap(target, staged, std::process::id(), true) {
                        Ok(()) => {
                            self.save(ctx);
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                        Err(error) => dialog.stage = UpdateStage::Failed(error),
                    }
                }
            }
            UpdateAction::Later => {
                if let (Some(target), UpdateStage::Ready(_)) = (&dialog.target, &dialog.stage) {
                    update::clean_staging(target);
                }
                keep = false;
            }
            UpdateAction::Skip => {
                self.settings.skipped_update = dialog.release.tag.clone();
                self.settings.save();
                keep = false;
            }
        }
        if keep {
            self.update = Some(dialog);
        }
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
                    from_cache,
                    already_converted,
                } => {
                    self.scanning = false;
                    self.scan_receiver = None;
                    self.scan_cancel = None;
                    self.scan_progress = None;
                    self.videos = videos;
                    reclassify(&mut self.videos, &self.settings.accepted_codecs);
                    // File names in Japanese or Korean need a system font.
                    theme::ensure_fallback_fonts(
                        ctx,
                        self.videos
                            .iter()
                            .flat_map(|video| [video.name.as_str(), video.folder.as_str()]),
                    );
                    self.warnings = warnings;
                    self.cancelled = cancelled;
                    self.summary = summarize(&self.videos, None);
                    self.selected.clear();
                    self.converted.clear();
                    // Videos an earlier session already converted into the output folder.
                    self.converted.extend(already_converted);
                    self.selection_anchor = None;
                    self.filter = FilterCategory::Convert;
                    self.page = Page::Results;
                    self.scan_completed_once = true;
                    if self.args.smoke_test {
                        self.close_after_scan_at = Some(Instant::now());
                    }
                    let status = if from_cache > 0 {
                        fotogramma_core::tr!(
                            "Nessun file viene modificato · dalla cache: {}",
                            "No file is modified · from the cache: {}",
                            format_int(from_cache)
                        )
                    } else {
                        tr_str("Nessun file viene modificato", "No file is modified").to_string()
                    };
                    self.set_status(status, Duration::from_secs(6));
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

    /// Ask for a folder and put it in the library field.
    fn browse_folder(&mut self) {
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

    /// Select every video that can be converted among the rows of the view.
    fn select_all_convertible(&mut self, visible: &[usize]) {
        let convertible = self.convertible_in(visible);
        self.selected.extend(convertible);
        self.selection_anchor = None;
    }

    /// A window that asks for an answer is open: the menu must not start
    /// anything underneath it.
    fn modal_open(&self) -> bool {
        self.conversion_dialog.is_some()
            || (self.session.is_some() && !self.session_minimized)
            || self.show_settings
            || self.show_about
            || self.update_notice.is_some()
            || self.show_update_prompt
            || self.details.is_some()
            || self.update.is_some()
            || self.close_confirm
            || self.resume_offer.is_some()
            || self.power_countdown.is_some()
    }

    fn menu_state(&self) -> MenuState {
        MenuState {
            idle: !self.scanning && !self.modal_open(),
            results: self.page == Page::Results,
            can_export: self.conversion_progress().0 > 0,
            can_convert: !self.selected_videos().is_empty(),
            converting: self.is_busy(),
        }
    }

    /// Carry out what the menu (or its shortcut) asked for.
    fn handle_menu_action(&mut self, action: MenuAction, ctx: &egui::Context) {
        let state = self.menu_state();
        if !state.enabled(action) {
            return;
        }
        match action {
            MenuAction::ChooseFolder => self.browse_folder(),
            MenuAction::Scan => self.start_scan(ctx),
            MenuAction::Export => self.export_videos(),
            MenuAction::ConvertSelected => {
                let selected = self.selected_videos();
                self.open_conversion_dialog(selected);
            }
            MenuAction::SelectAll => {
                let visible =
                    table::visible_indices(&self.videos, self.filter, &self.query, self.sort);
                self.select_all_convertible(&visible);
            }
            MenuAction::ClearSelection => {
                self.selected.clear();
                self.selection_anchor = None;
            }
            MenuAction::FocusSearch => self.focus_search = true,
            MenuAction::FilterAll => self.filter = FilterCategory::All,
            MenuAction::FilterConvert => self.filter = FilterCategory::Convert,
            MenuAction::FilterCompatible => self.filter = FilterCategory::Compatible,
            MenuAction::FilterErrors => self.filter = FilterCategory::Error,
            MenuAction::Settings => self.show_settings = true,
            MenuAction::CheckUpdates => self.start_update_check(ctx, true),
            MenuAction::About => self.show_about = true,
            MenuAction::Minimize => ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true)),
            MenuAction::ToggleMaximize => {
                let maximized = ctx.input(|input| input.viewport().maximized.unwrap_or(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
            }
            // Goes through the same close handling, which asks first when a job runs.
            MenuAction::CloseWindow | MenuAction::Quit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close)
            }
        }
        ctx.request_repaint();
    }

    fn start_scan(&mut self, ctx: &egui::Context) {
        if self.scanning {
            self.folder_error = Some(
                tr_str("È già in corso un’analisi.", "A scan is already running.").to_string(),
            );
            return;
        }
        let directory = self.directory.trim().to_string();
        if directory.is_empty() {
            self.folder_error = Some(
                tr_str(
                    "Inserisci o scegli una cartella da analizzare.",
                    "Enter or choose a folder to scan.",
                )
                .to_string(),
            );
            return;
        }
        let path = PathBuf::from(&directory);
        if !path.is_dir() {
            self.folder_error = Some(
                tr_str(
                    "La cartella indicata non esiste o non è accessibile.",
                    "The folder does not exist or is not accessible.",
                )
                .to_string(),
            );
            return;
        }
        self.folder_error = None;
        self.settings.directory = directory.clone();
        self.settings.save();
        self.scanning = true;
        self.scan_progress = Some((0, None, String::new()));
        let cancel = Arc::new(AtomicBool::new(false));
        self.scan_cancel = Some(cancel.clone());
        let options = worker::ScanOptions {
            cache_file: if self.settings.scan_cache {
                crate::settings::cache_file()
            } else {
                None
            },
            output_dir: self.settings.output_dir.clone(),
        };
        self.scan_receiver = Some(worker::spawn_scan(path, options, cancel, ctx.clone()));
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

    /// Paths of the videos in `visible` that can be converted.
    fn convertible_in(&self, visible: &[usize]) -> Vec<PathBuf> {
        visible
            .iter()
            .map(|index| &self.videos[*index])
            .filter(|video| video.category == fotogramma_core::scanner::Category::Convert)
            .filter(|video| !self.converted.contains_key(&video.path))
            .map(|video| video.path.clone())
            .collect()
    }

    /// Open the conversion window; while one conversion runs, what is chosen
    /// waits behind it.
    fn open_conversion_dialog(&mut self, videos: Vec<VideoInfo>) {
        if videos.is_empty() {
            return;
        }
        let mut dialog = ConversionDialog::new(videos, &self.settings);
        dialog.adds_to_queue = self.session.is_some();
        self.conversion_dialog = Some(dialog);
    }

    /// Whether a video is already in the running batch or in a waiting one.
    fn is_queued(&self, path: &PathBuf) -> bool {
        self.session
            .as_ref()
            .is_some_and(|session| session.sources.contains(path))
            || self
                .waiting
                .iter()
                .any(|plan| plan.videos.iter().any(|video| &video.path == path))
    }

    fn selected_videos(&self) -> Vec<VideoInfo> {
        self.videos
            .iter()
            .filter(|video| video.category == fotogramma_core::scanner::Category::Convert)
            .filter(|video| self.selected.contains(&video.path))
            .filter(|video| !self.is_queued(&video.path))
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
            let outcome = dialog.show(ctx);
            if dialog.presets_dirty {
                self.settings.presets = dialog.presets.clone();
                self.settings.save();
                dialog.presets_dirty = false;
            }
            match outcome {
                DialogOutcome::Start(plan) => {
                    self.conversion_dialog = None;
                    if self.session.is_some() {
                        for video in &plan.videos {
                            self.selected.remove(&video.path);
                        }
                        self.set_status(
                            fotogramma_core::tr!(
                                "Aggiunti alla coda: {} video",
                                "Added to the queue: {} videos",
                                format_int(plan.videos.len())
                            ),
                            Duration::from_secs(6),
                        );
                        self.waiting.push(*plan);
                    } else {
                        self.session_minimized = false;
                        self.start_conversion(*plan, ctx);
                    }
                }
                DialogOutcome::Cancel => {
                    self.conversion_dialog = None;
                }
                DialogOutcome::None => {}
            }
        }

        self.show_update_dialog(ctx);

        // The only question at first start; the queue offer comes after it.
        if self.show_update_prompt && self.resume_offer.is_none() {
            if let Some(allow) = dialogs::show_update_prompt(ctx) {
                self.show_update_prompt = false;
                self.settings.update_choice_made = true;
                self.settings.check_updates = allow;
                self.settings.save();
                if allow {
                    self.start_update_check(ctx, false);
                }
            }
        }

        // A queue that did not finish last time.
        if let Some(count) = self.resume_offer.as_ref().map(|queue| queue.videos.len()) {
            match dialogs::show_resume(ctx, count) {
                Some(dialogs::ResumeChoice::Resume) => {
                    if let Some(queue) = self.resume_offer.take() {
                        let plan = ConversionPlan {
                            videos: queue.videos,
                            profile: queue.profile,
                            destination_dir: queue.destination_dir,
                            container: queue.container,
                            replace_original: queue.replace_original,
                            options: crate::worker::ConversionOptions {
                                min_saving_percent: queue.min_saving_percent,
                                prevent_sleep: self.settings.prevent_sleep,
                                ..Default::default()
                            },
                            after: PowerAction::Nothing,
                        };
                        self.start_conversion(plan, ctx);
                    }
                }
                Some(dialogs::ResumeChoice::Discard) => {
                    self.resume_offer = None;
                    if let Some(file) = crate::settings::queue_file() {
                        PendingQueue::remove(&file);
                    }
                }
                None => {}
            }
        }

        // The queue ended: sleep or shut down after a countdown that can be cancelled.
        let queue_empty = self.waiting.is_empty();
        let ended = self
            .session
            .as_mut()
            .filter(|session| session.finished && !session.after_handled && queue_empty)
            .map(|session| {
                session.after_handled = true;
                let stopped = session.failed || session.cancel.load(Ordering::Relaxed);
                (session.after, stopped)
            });
        if let Some((action, stopped)) = ended {
            if action != PowerAction::Nothing && !stopped && self.power_countdown.is_none() {
                self.power_countdown = Some((action, Instant::now() + Duration::from_secs(60)));
            }
        }
        if let Some((action, at)) = self.power_countdown {
            let now = Instant::now();
            if now >= at {
                self.power_countdown = None;
                action.run();
            } else {
                match dialogs::show_power_countdown(ctx, action, (at - now).as_secs() + 1) {
                    Some(dialogs::PowerChoice::Now) => {
                        self.power_countdown = None;
                        action.run();
                    }
                    Some(dialogs::PowerChoice::Cancel) => self.power_countdown = None,
                    None => {}
                }
                ctx.request_repaint_after(Duration::from_millis(500));
            }
        }

        let mut close_session = false;
        let waiting = self.waiting.len();
        let minimized = self.session_minimized;
        if let Some(session) = self.session.as_mut().filter(|_| !minimized) {
            match dialogs::show_progress(ctx, session, waiting) {
                ProgressAction::Close => close_session = true,
                ProgressAction::Minimize => self.session_minimized = true,
                ProgressAction::OpenFolder => {
                    let destination = session.destination_dir().to_path_buf();
                    platform::open_folder(&destination);
                }
                ProgressAction::Cancel | ProgressAction::None => {}
            }
        }
        if close_session {
            self.mark_replaced_videos();
            let saved = self
                .session
                .as_ref()
                .and_then(|session| session.space)
                .filter(|space| space.is_saving() && space.delta() > 0);
            self.session = None;
            self.session_minimized = false;
            if let Some(space) = saved {
                self.set_status(
                    fotogramma_core::tr!(
                        "Spazio risparmiato: {} ({})",
                        "Space saved: {} ({})",
                        format_bytes(Some(space.delta())),
                        space.signed_percent()
                    ),
                    Duration::from_secs(10),
                );
            }
        }

        if self.show_settings {
            let outcome = dialogs::show_settings(ctx, &mut self.settings);
            if outcome.changed {
                self.apply_settings();
            }
            if outcome.check_now {
                self.start_update_check(ctx, true);
            }
            if outcome.clear_cache {
                if let Some(file) = crate::settings::cache_file() {
                    let _ = std::fs::remove_file(file);
                }
                self.set_status(
                    tr_str("Cache della scansione svuotata", "Scan cache cleared"),
                    Duration::from_secs(4),
                );
            }
            if outcome.closed {
                self.show_settings = false;
            }
        }

        if let Some(video) = self.details.clone() {
            if dialogs::show_details(ctx, &video) {
                self.details = None;
            }
        }

        if self.close_confirm {
            let running = dialogs::CloseContext {
                converting: self
                    .session
                    .as_ref()
                    .is_some_and(|session| session.is_running()),
                waiting: self.waiting.len(),
            };
            match dialogs::show_close_confirm(ctx, &running) {
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

    /// Persist the preferences and re-classify the loaded library, so a new
    /// accepted-codec list shows up in the table and the home page at once.
    fn apply_settings(&mut self) {
        self.settings.save();
        let before = fotogramma_core::i18n::lang();
        let language = Lang::resolve(&self.settings.language, system_locale().as_deref());
        set_lang(language);
        if language != before {
            // The engine label is built once: rebuild it in the new language.
            (self.engine_status, self.engine_ok) = engine_status();
            #[cfg(target_os = "macos")]
            if self.native_menu.is_some() {
                self.native_menu = Some(crate::native_menu::NativeMenu::install(&self.egui_ctx));
            }
        }
        reclassify(&mut self.videos, &self.settings.accepted_codecs);
        self.summary = summarize(&self.videos, None);
        let convertible: HashSet<PathBuf> = self
            .videos
            .iter()
            .filter(|video| video.category == fotogramma_core::scanner::Category::Convert)
            .map(|video| video.path.clone())
            .collect();
        self.selected.retain(|path| convertible.contains(path));
    }

    fn start_conversion(&mut self, mut plan: ConversionPlan, ctx: &egui::Context) {
        plan.options.queue_file = crate::settings::queue_file();
        plan.options.pause = Arc::new(AtomicBool::new(false));
        let cancel = Arc::new(AtomicBool::new(false));
        let receiver = worker::spawn_conversion(
            plan.videos.clone(),
            plan.profile.clone(),
            plan.destination_dir.clone(),
            plan.container.clone(),
            plan.replace_original,
            plan.options.clone(),
            cancel.clone(),
            ctx.clone(),
        );
        self.session = Some(ConversionSession::new(plan, receiver, cancel));
        ctx.request_repaint();
    }

    /// The batch that ran has finished: start the next waiting one, or drop the
    /// waiting ones if the user stopped the queue.
    fn advance_queue(&mut self, ctx: &egui::Context) {
        let Some(session) = &self.session else {
            return;
        };
        if !session.finished || self.waiting.is_empty() {
            return;
        }
        if session.cancel.load(Ordering::Relaxed) {
            self.waiting.clear();
            return;
        }
        self.session = None;
        let next = self.waiting.remove(0);
        self.start_conversion(next, ctx);
    }

    /// Keep the library in step with the conversions of the session. A video
    /// whose original was replaced moves to the compatible ones; one that was
    /// converted into a new file stays where it is but is marked as done, so
    /// it is not converted twice by accident.
    fn mark_replaced_videos(&mut self) {
        let Some(session) = &self.session else {
            return;
        };
        let accepted = self.settings.accepted_codecs.clone();
        let mut replaced_any = false;
        for result in session.results.iter().filter(|result| result.success) {
            if self.converted.get(&result.source) == Some(&result.destination) {
                continue;
            }
            self.converted
                .insert(result.source.clone(), result.destination.clone());
            self.selected.remove(&result.source);
            if result.replaced {
                if let Some(video) = self.videos.iter_mut().find(|v| v.path == result.source) {
                    video.category = classify_codec("hevc", &accepted);
                    video.codec = Some("hevc".to_string());
                    video.error = None;
                    if result.new_size.is_some() {
                        video.size = result.new_size;
                    }
                }
                replaced_any = true;
            }
        }
        if replaced_any {
            self.summary = summarize(&self.videos, None);
        }
    }

    /// Videos still to convert, how many were already converted in this
    /// session, and the bytes still to process.
    fn conversion_progress(&self) -> (usize, usize, u64) {
        let (mut pending, mut done, mut bytes) = (0, 0, 0);
        for video in &self.videos {
            if video.category != fotogramma_core::scanner::Category::Convert {
                continue;
            }
            if self.converted.contains_key(&video.path) {
                done += 1;
            } else {
                pending += 1;
                bytes += video.size.unwrap_or(0);
            }
        }
        (pending, done, bytes)
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

    fn header(&mut self, ui: &mut egui::Ui) {
        let mut open_settings = false;
        ui.horizontal(|ui| {
            if let Some(logo) = &self.logo {
                ui.add(egui::Image::new(logo).fit_to_exact_size(Vec2::splat(46.0)));
                ui.add_space(4.0);
            }
            ui.vertical(|ui| {
                ui.label(RichText::new("Fotogramma").size(theme::BRAND_SIZE).strong());
                ui.label(
                    RichText::new("H.265 cinema library")
                        .small()
                        .color(theme::MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add(theme::secondary_button(tr_str("Impostazioni", "Settings")))
                    .clicked()
                {
                    open_settings = true;
                }
                ui.add_space(8.0);
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
                    RichText::new(tr_str(
                        "Solo sul PC · Solo lettura",
                        "Local only · Read-only",
                    ))
                    .small()
                    .color(theme::MUTED),
                );
            });
        });
        if open_settings {
            self.show_settings = true;
        }
    }

    fn control_deck(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        theme::card_frame().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(RichText::new(tr_str("Cartella della libreria", "Library folder")).strong());
            ui.label(
                RichText::new(tr_str(
                    "Analisi ricorsiva di MP4, MKV e WebM. Nessuna modifica ai file.",
                    "Recursive scan of MP4, MKV and WebM. No file is modified.",
                ))
                .small()
                .color(theme::MUTED),
            );
            ui.add_space(2.0);
            let mut browse = false;
            let mut analyze = false;
            let mut submit = false;
            ui.horizontal(|ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    analyze = ui
                        .add_enabled(
                            !self.scanning,
                            theme::primary_button(tr_str("Analizza libreria", "Scan library")),
                        )
                        .clicked();
                    browse = ui
                        .add_enabled(
                            !self.scanning,
                            theme::secondary_button(tr_str("Sfoglia", "Browse")),
                        )
                        .clicked();
                    let edit = ui.add_sized(
                        [ui.available_width(), 34.0],
                        egui::TextEdit::singleline(&mut self.directory)
                            .font(theme::mono(11.0))
                            .hint_text(tr_str(
                                "Es. /Filmati oppure D:\\Video",
                                "E.g. /Movies or D:\\Video",
                            )),
                    );
                    submit = edit.lost_focus() && ui.input(|input| input.key_pressed(Key::Enter));
                });
            });
            if submit || analyze {
                self.start_scan(ctx);
            }
            if browse {
                self.browse_folder();
            }

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
                            let cancelled = ui
                                .add(theme::secondary_button(tr_str("Interrompi", "Stop")))
                                .clicked();
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
                                        // The counter box has a fixed width, so the bar
                                        // never moves when the file names change.
                                        let counter_box =
                                            (ui.available_width() * 0.4).clamp(180.0, 420.0);
                                        let bar_width =
                                            (ui.available_width() - counter_box - 8.0).max(60.0);
                                        ui.add(
                                            egui::ProgressBar::new(fraction)
                                                .desired_height(8.0)
                                                .desired_width(bar_width)
                                                .fill(theme::CORAL),
                                        );
                                        ui.allocate_ui_with_layout(
                                            Vec2::new(counter_box, 14.0),
                                            Layout::left_to_right(Align::Center),
                                            |ui| {
                                                ui.add(
                                                    egui::Label::new(
                                                        RichText::new(counter)
                                                            .small()
                                                            .color(theme::MUTED),
                                                    )
                                                    .truncate(),
                                                );
                                            },
                                        );
                                    }
                                    None => {
                                        ui.add(egui::Spinner::new());
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(fotogramma_core::tr!(
                                                    "Video trovati: {} · {current}",
                                                    "Videos found: {} · {current}",
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
                            RichText::new(tr_str("Pronta per H.265.", "Ready for H.265."))
                                .size(theme::HERO_SIZE)
                                .strong(),
                        );
                        ui.add_space(2.0);
                        ui.label(
                            RichText::new(
                                tr_str(
                                "Scegli una cartella e avvia l’analisi. Fotogramma separa i video già pronti da quelli che potresti convertire, senza spostare o modificare nulla.",
                                "Choose a folder and start the scan. Fotogramma separates the videos that are already ready from those you could convert, without moving or changing anything.",
                            ),
                            )
                            .color(theme::MUTED),
                        );
                        ui.add_space(18.0);
                        for (number, heading, description) in [
                            ("01", tr_str("Scansione", "Scan"), tr_str("Lettura del primo flusso video tramite ffprobe.", "Reads the first video stream with ffprobe.")),
                            ("02", tr_str("Selezione", "Select"), tr_str("Filtri, ricerca e ordinamento per isolare i candidati.", "Filters, search and sorting to isolate the candidates.")),
                            ("03", tr_str("Esportazione", "Export"), tr_str("Un file di testo con i soli percorsi da convertire.", "A text file with only the paths to convert.")),
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
                    for (key, text) in KNOWN_CODECS {
                        let accepted = self.settings.accepted_codecs.iter().any(|value| value == key);
                        chip(ui, text, accepted);
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
                    RichText::new(tr_str("La tua libreria", "Your library"))
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
                    .add(theme::primary_button(tr_str(
                        "Esporta da convertire",
                        "Export to convert",
                    )))
                    .clicked()
                {
                    self.export_videos();
                }
            });
        });
        ui.add_space(6.0);

        let (pending, done, pending_bytes) = self.conversion_progress();
        let spacing = 10.0;
        let width = ((ui.available_width() - spacing * 3.0) / 5.0).max(100.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = spacing;
            let cards = [
                (
                    "all",
                    self.summary.total,
                    tr_str("Video presenti", "Videos found"),
                    fotogramma_core::tr!(
                        "{} analizzati",
                        "{} analyzed",
                        format_bytes(Some(self.summary.total_bytes))
                    ),
                    theme::LAVENDER,
                    width,
                ),
                (
                    "convert",
                    self.summary.to_convert,
                    tr_str("Da convertire", "To convert"),
                    if done > 0 {
                        fotogramma_core::tr!(
                            "{} da elaborare · {} già fatti",
                            "{} to process · {} already done",
                            format_bytes(Some(pending_bytes)),
                            format_int(done)
                        )
                    } else {
                        fotogramma_core::tr!(
                            "{} da elaborare",
                            "{} to process",
                            format_bytes(Some(pending_bytes))
                        )
                    },
                    theme::CORAL,
                    width * 2.0,
                ),
                (
                    "compatible",
                    self.summary.compatible,
                    tr_str("Già compatibili", "Compatible"),
                    accepted_summary(&self.settings.accepted_codecs),
                    theme::MINT,
                    width,
                ),
                (
                    "error",
                    self.summary.errors,
                    tr_str("Non leggibili", "Unreadable"),
                    tr_str("Da controllare", "To check").to_string(),
                    theme::ERROR,
                    width,
                ),
            ];
            for (key, value, label, detail, tone, card_width) in cards {
                let filter = match key {
                    "convert" => FilterCategory::Convert,
                    "compatible" => FilterCategory::Compatible,
                    "error" => FilterCategory::Error,
                    _ => FilterCategory::All,
                };
                let selected = self.filter == filter;
                if stat_card(ui, card_width, 66.0, value, label, &detail, tone, selected).clicked()
                {
                    self.filter = filter;
                }
            }
        });
        ui.add_space(10.0);

        let caption = if total > 0 {
            fotogramma_core::tr!(
                "{}% da convertire · {}% compatibili",
                "{}% to convert · {}% compatible",
                (self.summary.to_convert as f32 / total as f32 * 100.0).round() as u32,
                (self.summary.compatible as f32 / total as f32 * 100.0).round() as u32
            )
        } else {
            tr_str("Nessun video trovato", "No videos found").to_string()
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
                messages.push(tr_str(
                    "Analisi interrotta: i risultati sono parziali.",
                    "Scan stopped: the results are partial.",
                ));
            }
            if !self.warnings.is_empty() {
                messages.push(tr_str(
                    "cartelle non accessibili: il riepilogo può essere incompleto.",
                    "folders not accessible: the summary may be incomplete.",
                ));
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

        let insight = if pending > 0 && done > 0 {
            fotogramma_core::tr!(
                "File da convertire: {} · già convertiti: {}. Selezionali nella tabella per convertirli direttamente.",
                "Files to convert: {} · already converted: {}. Select them in the table to convert them directly.",
                format_int(pending),
                format_int(done)
            )
        } else if pending > 0 {
            fotogramma_core::tr!(
                "File da convertire: {}. Selezionali nella tabella per convertirli direttamente.",
                "Files to convert: {}. Select them in the table to convert them directly.",
                format_int(pending)
            )
        } else if done > 0 {
            tr_str(
                "Tutto convertito. Dove non hai sostituito l’originale, il nuovo file è nella cartella di output.",
                "Everything is converted. Where you did not replace the original, the new file is in the output folder.",
            )
            .to_string()
        } else {
            fotogramma_core::tr!(
                "Tutti i file sono già compatibili ({}): non serve alcuna conversione.",
                "All files are already compatible ({}): no conversion needed.",
                accepted_summary(&self.settings.accepted_codecs)
            )
        };
        ui.label(RichText::new(insight).small().color(theme::MUTED));

        ui.horizontal(|ui| {
            for (filter, label, tone) in [
                (FilterCategory::All, tr_str("Tutti", "All"), theme::LAVENDER),
                (
                    FilterCategory::Convert,
                    tr_str("Da convertire", "To convert"),
                    theme::CORAL,
                ),
                (
                    FilterCategory::Compatible,
                    tr_str("Compatibili", "Compatible"),
                    theme::MINT,
                ),
                (
                    FilterCategory::Error,
                    tr_str("Errori", "Errors"),
                    theme::ERROR,
                ),
            ] {
                if filter_pill(ui, label, tone, self.filter == filter).clicked() {
                    self.filter = filter;
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let search = ui.add_sized(
                    [340.0, 34.0],
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text(tr_str("Cerca nella libreria…", "Search the library…")),
                );
                if std::mem::take(&mut self.focus_search) {
                    search.request_focus();
                }
            });
        });

        let visible = table::visible_indices(&self.videos, self.filter, &self.query, self.sort);
        let convertible = self.convertible_in(&visible);
        let selected = self.selected_videos();
        let all_selected =
            !convertible.is_empty() && convertible.iter().all(|path| self.selected.contains(path));
        ui.horizontal(|ui| {
            let label = if selected.is_empty() {
                tr_str(
                    "Seleziona uno o più video da convertire per vedere le azioni",
                    "Select one or more videos to convert to see the actions",
                )
                .to_string()
            } else {
                fotogramma_core::tr!(
                    "Selezionati da convertire: {}",
                    "Selected to convert: {}",
                    format_int(selected.len())
                )
            };
            ui.label(RichText::new(label).small().color(theme::MUTED));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui
                    .add_enabled(
                        !selected.is_empty(),
                        theme::primary_button(tr_str("Converti selezionati", "Convert selected")),
                    )
                    .on_hover_text(tr_str(
                        "Crea nuovi file H.265 senza modificare gli originali",
                        "Creates new H.265 files without changing the originals",
                    ))
                    .clicked()
                {
                    self.open_conversion_dialog(selected.clone());
                }
                // Select everything that can be converted in the current view
                // (so the filter and the search narrow it), or clear it again.
                if all_selected || (!selected.is_empty() && convertible.is_empty()) {
                    if ui
                        .add(theme::secondary_button(tr_str(
                            "Deseleziona",
                            "Clear selection",
                        )))
                        .clicked()
                    {
                        self.selected.clear();
                        self.selection_anchor = None;
                    }
                } else if ui
                    .add_enabled(
                        !convertible.is_empty(),
                        theme::secondary_button(fotogramma_core::tr!(
                            "Seleziona tutti ({})",
                            "Select all ({})",
                            format_int(convertible.len())
                        )),
                    )
                    .on_hover_text(tr_str(
                        "Seleziona tutti i video da convertire nella vista attuale",
                        "Selects every video to convert in the current view",
                    ))
                    .clicked()
                {
                    self.select_all_convertible(&visible);
                }
            });
        });

        let mut action = None;
        ui.scope(|ui| {
            // What is left of the panel, but never so little that the table is
            // unreadable: then the whole page scrolls.
            let left = self.panel_bottom - ui.cursor().top();
            let height = (left - 26.0).max(260.0);
            ui.allocate_ui(Vec2::new(ui.available_width(), height), |ui| {
                action = table::show(
                    ui,
                    &self.videos,
                    &visible,
                    &self.selected,
                    &self.converted,
                    &mut self.sort,
                );
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
            RichText::new(fotogramma_core::tr!(
                "Video in questa vista: {}",
                "Videos in this view: {}",
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
                RichText::new(tr_str("L’analisi si è fermata", "The scan stopped"))
                    .size(theme::PAGE_TITLE_SIZE)
                    .strong(),
            );
            ui.label(
                RichText::new(fotogramma_core::tr!(
                    "{message}\n\nControlla la cartella e che ffprobe sia disponibile, poi riprova.", "{message}\n\nCheck the folder and that ffprobe is available, then try again."
                ))
                .color(theme::MUTED),
            );
            ui.add_space(8.0);
            if ui.add(theme::primary_button(tr_str("Riprova", "Try again"))).clicked() {
                self.start_scan(ctx);
            }
        });
    }

    /// The videos still to convert, as plain paths, CSV or JSON: the format
    /// follows the extension chosen in the save dialog.
    fn export_videos(&mut self) {
        let pending: Vec<VideoInfo> = self
            .videos
            .iter()
            .filter(|video| video.category == fotogramma_core::scanner::Category::Convert)
            .filter(|video| !self.converted.contains_key(&video.path))
            .cloned()
            .collect();
        if pending.is_empty() {
            self.set_status(
                tr_str("Nessun video da convertire", "No videos to convert"),
                Duration::from_secs(4),
            );
            return;
        }
        let Some(target) = rfd::FileDialog::new()
            .set_file_name("fotogramma-da-convertire.txt")
            .set_directory(home_directory())
            .add_filter(tr_str("Elenco di percorsi", "List of paths"), &["txt"])
            .add_filter("CSV", &["csv"])
            .add_filter("JSON", &["json"])
            .save_file()
        else {
            return;
        };
        let format = Format::from_extension(
            &target
                .extension()
                .map(|extension| extension.to_string_lossy().into_owned())
                .unwrap_or_default(),
        );
        match std::fs::write(&target, export::render(&pending, format)) {
            Ok(()) => self.set_status(
                fotogramma_core::tr!(
                    "Video esportati: {} · {}",
                    "Videos exported: {} · {}",
                    format_int(pending.len()),
                    target.display()
                ),
                Duration::from_secs(7),
            ),
            Err(error) => self.set_status(
                fotogramma_core::tr!(
                    "Esportazione non riuscita: {error}",
                    "Export failed: {error}"
                ),
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
        self.poll_update();
        let mut menu_actions: Vec<MenuAction> = Vec::new();
        #[cfg(target_os = "macos")]
        if self.native_menu.is_some() {
            let state = self.menu_state();
            if let Some(native) = self.native_menu.as_mut() {
                native.update(&state);
            }
            menu_actions.extend(crate::native_menu::NativeMenu::take_actions());
        }
        if !cfg!(target_os = "macos") {
            menu_actions.extend(menu::shortcut_pressed(&ctx));
        }
        if let Some(session) = self.session.as_mut() {
            session.poll();
        }
        self.mark_replaced_videos();
        self.advance_queue(&ctx);

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

        if self.in_window_menu {
            let state = self.menu_state();
            egui::Panel::top("menu-bar")
                .show_separator_line(false)
                .frame(
                    egui::Frame::NONE
                        .fill(theme::BG_DEEP)
                        .inner_margin(egui::Margin::symmetric(12, 2)),
                )
                .show(ui, |ui| {
                    menu_actions.extend(menu::show_bar(ui, &state));
                });
        }

        if self.session.is_some() && self.session_minimized {
            let waiting = self.waiting.len();
            let mut bar = dialogs::BarAction::None;
            if let Some(session) = self.session.as_ref() {
                egui::Panel::bottom("conversion-bar")
                    .show_separator_line(false)
                    .frame(
                        egui::Frame::NONE
                            .fill(theme::BG_DEEP)
                            .inner_margin(egui::Margin::symmetric(24, 8)),
                    )
                    .show(ui, |ui| {
                        bar = dialogs::show_background_bar(ui, session, waiting);
                    });
            }
            match bar {
                dialogs::BarAction::Show => self.session_minimized = false,
                dialogs::BarAction::Pause => {
                    if let Some(session) = &self.session {
                        let paused = session.pause.load(Ordering::Relaxed);
                        session.pause.store(!paused, Ordering::Relaxed);
                    }
                }
                dialogs::BarAction::Stop => {
                    if let Some(session) = &self.session {
                        session.cancel();
                    }
                }
                dialogs::BarAction::None => {}
            }
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
                self.panel_bottom = ui.max_rect().bottom();
                // A window too small for everything scrolls instead of
                // cutting the text off.
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
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
            });

        for action in menu_actions {
            self.handle_menu_action(action, &ctx);
        }
        if let Some(notice) = self.update_notice.clone() {
            if dialogs::show_update_notice(&ctx, &notice) {
                self.update_notice = None;
            }
        }
        if self.show_about && crate::dialogs::show_about(&ctx, self.logo.as_ref()) {
            self.show_about = false;
        }
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

/// The operating system language, as a locale tag. Tests use a fixed value so
/// the result never depends on the machine running them.
fn system_locale() -> Option<String> {
    if cfg!(test) {
        return Some("it-IT".to_string());
    }
    sys_locale::get_locale()
}

/// The update check is the app's only network request. It never runs in the
/// automated checks, and `FOTOGRAMMA_NO_UPDATE_CHECK` switches it off.
fn updates_allowed(args: &Args) -> bool {
    !cfg!(test) && !args.smoke_test && std::env::var_os("FOTOGRAMMA_NO_UPDATE_CHECK").is_none()
}

fn engine_status() -> (String, bool) {
    match ffprobe_path() {
        Ok(path) => match ffprobe_version(&path) {
            Some(version) => {
                let parts: Vec<&str> = version.split_whitespace().collect();
                if parts.len() > 2 {
                    (format!("ffprobe {}", parts[2]), true)
                } else {
                    (
                        tr_str("ffprobe non disponibile", "ffprobe unavailable").to_string(),
                        false,
                    )
                }
            }
            None => (
                tr_str("ffprobe non disponibile", "ffprobe unavailable").to_string(),
                false,
            ),
        },
        Err(_) => (
            tr_str("ffprobe mancante", "ffprobe missing").to_string(),
            false,
        ),
    }
}

fn home_directory() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
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

fn filter_pill(ui: &mut egui::Ui, label: &str, tone: Color32, selected: bool) -> egui::Response {
    let text_color = if selected {
        theme::CORAL_INK
    } else {
        theme::LAVENDER
    };
    ui.add(
        egui::Button::new(RichText::new(label).color(text_color))
            .fill(if selected { tone } else { theme::RAISED })
            .stroke(Stroke::new(
                1.0,
                if selected { tone } else { theme::INPUT_BORDER },
            ))
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
    tone: Color32,
    selected: bool,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), Sense::click());
    // The selected card is filled with its own tone; the others keep a thin
    // outline in that tone so each card stays recognizable.
    let fill = if selected { tone } else { theme::SURFACE };
    let stroke = if selected {
        Stroke::new(2.0, theme::CORAL_INK)
    } else {
        Stroke::new(1.0, tone.gamma_multiply(0.45))
    };
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(12), fill);
    painter.rect_stroke(rect, CornerRadius::same(12), stroke, StrokeKind::Inside);
    let ink = if selected {
        theme::CORAL_INK
    } else {
        theme::TEXT
    };
    let muted = if selected {
        theme::CORAL_INK.gamma_multiply(0.8)
    } else {
        theme::MUTED
    };
    painter.text(
        rect.left_top() + Vec2::new(16.0, 13.0),
        Align2::LEFT_TOP,
        format!("{} · {}", format_int(value), label),
        FontId::proportional(16.0),
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

/// Short list of the accepted codecs, such as "H.265 oppure AV1".
fn accepted_summary(accepted: &[String]) -> String {
    let labels: Vec<&str> = KNOWN_CODECS
        .iter()
        .filter(|(key, _)| accepted.iter().any(|value| value == key))
        .map(|(_, label)| label.split(" · ").next().unwrap_or(label))
        .collect();
    if labels.is_empty() {
        tr_str("Nessun codec accettato", "No accepted codec").to_string()
    } else {
        labels.join(tr_str(" oppure ", " or "))
    }
}

fn load_logo(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    const PNG: &[u8] = include_bytes!("../../../deployment/Fotogramma-window.png");
    let image = image::load_from_memory(PNG).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw());
    Some(ctx.load_texture("logo", color, egui::TextureOptions::LINEAR))
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

    /// The language is process-wide, so every rendering test takes this lock
    /// and starts in Italian; the English test cannot disturb the others.
    static LANG_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    pub(super) struct LangGuard(#[allow(dead_code)] std::sync::MutexGuard<'static, ()>);

    pub(super) fn lang_guard() -> LangGuard {
        let guard = LANG_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        set_lang(Lang::It);
        LangGuard(guard)
    }

    impl Drop for LangGuard {
        fn drop(&mut self) {
            set_lang(Lang::It);
        }
    }

    fn demo_video(
        name: &str,
        codec: &str,
        category: Category,
        size: u64,
        duration: f64,
        error: Option<&str>,
    ) -> VideoInfo {
        VideoInfo {
            hdr: false,
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
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1280.0, 820.0))
            .build_eframe(demo_app);
        harness.run();
        harness.snapshot("main-window");
    }

    #[test]
    fn the_scan_row_stays_inside_the_window() {
        let _lang = lang_guard();
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
            let stop = harness.get_by_label(tr_str("Interrompi", "Stop")).rect();
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
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1280.0, 820.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.selected.insert(PathBuf::from("/Filmati/Tramonto.mkv"));
                app.selected
                    .insert(PathBuf::from("/Filmati/Gita al lago.mp4"));
                let selected = app.selected_videos();
                app.conversion_dialog = Some(ConversionDialog::new(selected, &app.settings));
                app
            });
        harness.run();
        harness.snapshot("conversion-dialog");
    }
}

#[cfg(test)]
mod small_snapshot_tests {
    use super::snapshot_tests::{demo_app, lang_guard};
    use super::*;

    #[test]
    fn main_window_small_renders() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        harness.snapshot("main-window-small");
    }

    #[test]
    fn conversion_dialog_small_renders() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.selected.insert(PathBuf::from("/Filmati/Tramonto.mkv"));
                app.selected
                    .insert(PathBuf::from("/Filmati/Gita al lago.mp4"));
                let selected = app.selected_videos();
                app.conversion_dialog = Some(ConversionDialog::new(selected, &app.settings));
                app
            });
        harness.run();
        harness.snapshot("conversion-dialog-small");
    }

    #[test]
    fn the_profile_comparison_explains_the_choice() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.selected.insert(PathBuf::from("/Filmati/Tramonto.mkv"));
                let selected = app.selected_videos();
                app.conversion_dialog = Some(ConversionDialog::new(selected, &app.settings));
                app
            });
        harness.run();
        harness.get_by_label("Quale profilo scegliere?").click();
        harness.run();
        harness.step();
        for _ in 0..10 {
            harness.step();
        }
        harness.snapshot("conversion-dialog-profiles");
    }

    #[test]
    fn accepted_codecs_reclassify_the_loaded_library() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let app = harness.state_mut();
        assert!(app.summary.compatible > 0 && app.summary.to_convert > 0);
        app.settings.accepted_codecs = vec!["h264".to_string()];
        app.apply_settings();
        assert_eq!(
            app.summary.compatible,
            app.videos
                .iter()
                .filter(|v| v.codec.as_deref() == Some("h264") && v.error.is_none())
                .count()
        );
        assert!(app
            .videos
            .iter()
            .filter(|v| v.codec.as_deref() == Some("hevc"))
            .all(|v| v.category == fotogramma_core::scanner::Category::Convert));
        assert_eq!(app.summary.errors, 1);
    }

    #[test]
    fn the_interface_follows_the_language_setting() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.settings.language = "en".to_string();
                app.apply_settings();
                app
            });
        harness.run();
        harness.get_by_label("Settings");
        harness.get_by_label("Scan library");
        harness.get_by_label("Convert selected");
        assert!(harness.query_by_label("Impostazioni").is_none());
        assert!(harness.query_by_label("Analizza libreria").is_none());
    }

    fn release(tag: &str) -> fotogramma_core::update::Release {
        fotogramma_core::update::Release {
            tag: tag.to_string(),
            version: tag.trim_start_matches('v').to_string(),
            notes: "## What's new\n\n- Self-update.\n- A DMG installer.".to_string(),
            page_url: "https://github.com/JohnnyTaxs/fotogramma/releases/tag/v9.9.9".to_string(),
            package: Some(fotogramma_core::update::Package {
                name: "Fotogramma-Darwin-arm64.zip".to_string(),
                url: "https://github.com/JohnnyTaxs/fotogramma/releases/download/v9.9.9/x.zip"
                    .to_string(),
                size: 10,
            }),
            sums_url: None,
        }
    }

    fn offer(app: &mut FotogrammaApp, tag: &str, manual: bool) {
        let (sender, receiver) = std::sync::mpsc::channel();
        sender
            .send(CheckMessage::Available(Box::new(release(tag))))
            .unwrap();
        app.update_check = Some(receiver);
        app.update_manual = manual;
        app.poll_update();
    }

    #[test]
    fn a_newer_release_opens_the_update_dialog() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let app = harness.state_mut();
        offer(app, "v9.9.9", false);
        assert!(app.update.is_some());
        assert!(
            !app.update.as_ref().unwrap().can_install(),
            "no checksums, no automatic install"
        );
    }

    #[test]
    fn a_skipped_release_stays_hidden_unless_checked_by_hand() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let app = harness.state_mut();
        app.settings.skipped_update = "v9.9.9".to_string();
        offer(app, "v9.9.9", false);
        assert!(app.update.is_none());
        offer(app, "v9.9.9", true);
        assert!(app.update.is_some());
    }

    #[test]
    fn select_all_takes_every_convertible_video_in_the_view() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        harness.get_by_label("Seleziona tutti (2)").click();
        harness.run();
        assert_eq!(harness.state().selected.len(), 2);
        // Once everything is selected the same button clears it.
        harness.get_by_label("Deseleziona").click();
        harness.run();
        assert!(harness.state().selected.is_empty());
    }

    #[test]
    fn select_all_follows_the_filter_and_the_search() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.query = "tramonto".to_string();
                app
            });
        harness.run();
        harness.get_by_label("Seleziona tutti (1)").click();
        harness.run();
        let selected = &harness.state().selected;
        assert_eq!(selected.len(), 1);
        assert!(selected.iter().all(|path| path.ends_with("Tramonto.mkv")));
    }

    #[test]
    fn select_all_is_disabled_when_nothing_can_be_converted() {
        use egui_kittest::kittest::{NodeT, Queryable};
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.filter = FilterCategory::Compatible;
                app
            });
        harness.run();
        assert!(harness
            .get_by_label("Seleziona tutti (0)")
            .accesskit_node()
            .is_disabled());
    }

    fn done(name: &str, replaced: bool) -> crate::worker::ConversionResult {
        let source = PathBuf::from(format!("/Filmati/{name}"));
        crate::worker::ConversionResult {
            destination: if replaced {
                source.clone()
            } else {
                PathBuf::from(format!("/Filmati/Fotogramma-H265/{name}"))
            },
            source,
            success: true,
            message: String::new(),
            cancelled: false,
            verified: replaced,
            replaced,
            original_size: Some(573_500_000),
            new_size: Some(201_000_000),
            skipped: false,
        }
    }

    fn finish_with(app: &mut FotogrammaApp, results: Vec<crate::worker::ConversionResult>) {
        let (sender, receiver) = std::sync::mpsc::channel();
        let plan = ConversionPlan {
            videos: Vec::new(),
            profile: fotogramma_core::conversion::profile("balanced").unwrap(),
            destination_dir: PathBuf::from("/Filmati/Fotogramma-H265"),
            container: "mkv".to_string(),
            replace_original: false,
            options: crate::worker::ConversionOptions::default(),
            after: PowerAction::Nothing,
        };
        let session = ConversionSession::new(plan, receiver, Arc::new(AtomicBool::new(false)));
        for result in &results {
            sender
                .send(crate::worker::ConversionMessage::FileCompleted(Box::new(
                    result.clone(),
                )))
                .unwrap();
        }
        sender
            .send(crate::worker::ConversionMessage::Completed(results))
            .unwrap();
        app.session = Some(session);
    }

    #[test]
    fn a_replaced_original_moves_to_the_compatible_videos() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let app = harness.state_mut();
        app.selected.insert(PathBuf::from("/Filmati/Tramonto.mkv"));
        let (convert, compatible) = (app.summary.to_convert, app.summary.compatible);
        finish_with(app, vec![done("Tramonto.mkv", true)]);
        harness.run();
        let app = harness.state();
        assert_eq!(app.summary.to_convert, convert - 1);
        assert_eq!(app.summary.compatible, compatible + 1);
        let video = app
            .videos
            .iter()
            .find(|v| v.name == "Tramonto.mkv")
            .unwrap();
        assert_eq!(
            video.category,
            fotogramma_core::scanner::Category::Compatible
        );
        assert_eq!(video.codec.as_deref(), Some("hevc"));
        assert_eq!(video.size, Some(201_000_000), "the size is the new one");
        assert!(app.selected.is_empty(), "a finished video is deselected");
    }

    #[test]
    fn a_new_file_marks_the_original_as_already_converted() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.filter = FilterCategory::Convert;
                app
            });
        harness.run();
        let app = harness.state_mut();
        let convert = app.summary.to_convert;
        finish_with(app, vec![done("Tramonto.mkv", false)]);
        harness.run();
        let app = harness.state();
        // It stays among the videos to convert (the original is untouched) ...
        assert_eq!(app.summary.to_convert, convert);
        assert!(app
            .converted
            .contains_key(std::path::Path::new("/Filmati/Tramonto.mkv")));
        // ... but it says so, and "select all" no longer takes it.
        harness.get_by_label("Già convertito");
        harness.get_by_label("Seleziona tutti (1)");
    }

    #[test]
    fn everything_converted_is_said_in_the_hint() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let app = harness.state_mut();
        finish_with(
            app,
            vec![done("Tramonto.mkv", false), done("Gita al lago.mp4", false)],
        );
        harness.run();
        let app = harness.state();
        assert_eq!(app.conversion_progress().0, 0);
        assert_eq!(app.conversion_progress().1, 2);
    }

    #[test]
    fn release_notes_are_rendered_not_shown_raw() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                let mut release = release("v9.9.9");
                release.notes = include_str!("../../../docs/releases/v2.6.0.md").to_string();
                app.update = Some(UpdateDialog::new(release, None));
                app
            });
        harness.run();
        assert!(
            harness.query_by_label_contains("**").is_none(),
            "bold markers"
        );
        assert!(
            harness.query_by_label_contains("## ").is_none(),
            "heading markers"
        );
        assert!(
            harness.query_by_label_contains("`").is_none(),
            "code markers"
        );
        harness.get_by_label_contains("Space saved.");
    }

    #[test]
    fn notes_with_unsupported_characters_do_not_crash_the_window() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                let mut release = release("v9.9.9");
                release.notes =
                    "# Novità → 日本語 🎉\n\n- **Nuovo** ✔ ✓ è ’a’ ‘b’ “c” … – — × · © ½ ← ↑ → ↓ ⇒ test\n"
                        .to_string();
                app.update = Some(UpdateDialog::new(release, None));
                app
            });
        harness.run();
    }

    #[test]
    fn japanese_file_names_are_drawn_once_a_system_font_is_found() {
        if !theme::cjk_font_available() {
            eprintln!("skipped: no system font for Japanese on this machine");
            return;
        }
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                let mut video = app.videos[3].clone();
                video.name = "進撃の巨人 第1話.mkv".to_string();
                video.relative_path = "アニメ/進撃の巨人 第1話.mkv".to_string();
                video.folder = "アニメ".to_string();
                video.path = PathBuf::from("/Filmati/アニメ/進撃の巨人 第1話.mkv");
                app.videos.push(video);
                theme::ensure_fallback_fonts(
                    &cc.egui_ctx,
                    app.videos
                        .iter()
                        .flat_map(|video| [video.name.as_str(), video.folder.as_str()]),
                );
                app.filter = FilterCategory::All;
                app.query = "進撃".to_string();
                app
            });
        // The test harness panics on a glyph nothing can draw, so reaching the
        // end proves every character of the names has a font.
        harness.run();
    }

    #[test]
    fn the_menu_changes_the_view_and_the_selection() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        app.handle_menu_action(MenuAction::FilterCompatible, &ctx);
        assert_eq!(app.filter, FilterCategory::Compatible);
        app.handle_menu_action(MenuAction::FilterConvert, &ctx);
        assert_eq!(app.filter, FilterCategory::Convert);
        app.handle_menu_action(MenuAction::SelectAll, &ctx);
        assert_eq!(app.selected.len(), 2, "both videos to convert");
        app.handle_menu_action(MenuAction::ClearSelection, &ctx);
        assert!(app.selected.is_empty());
        app.handle_menu_action(MenuAction::FocusSearch, &ctx);
        assert!(app.focus_search);
        app.handle_menu_action(MenuAction::Settings, &ctx);
        assert!(app.show_settings);
        app.show_settings = false;
        app.handle_menu_action(MenuAction::About, &ctx);
        assert!(app.show_about);
    }

    #[test]
    fn the_menu_does_nothing_underneath_an_open_window() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        app.show_settings = true;
        app.handle_menu_action(MenuAction::FilterErrors, &ctx);
        assert_eq!(
            app.filter,
            FilterCategory::All,
            "the Settings window has the focus"
        );
        app.handle_menu_action(MenuAction::Scan, &ctx);
        assert!(!app.scanning);
    }

    #[test]
    fn convert_selected_in_the_menu_opens_the_dialog_only_with_a_selection() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        app.handle_menu_action(MenuAction::ConvertSelected, &ctx);
        assert!(app.conversion_dialog.is_none(), "nothing is selected");
        app.selected.insert(PathBuf::from("/Filmati/Tramonto.mkv"));
        app.handle_menu_action(MenuAction::ConvertSelected, &ctx);
        assert!(app.conversion_dialog.is_some());
    }

    #[test]
    fn the_in_window_menu_has_every_menu_and_its_items_work() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.in_window_menu = true;
                app
            });
        harness.run();
        for title in ["File", "Modifica", "Vista", "Finestra", "Aiuto"] {
            harness.get_by_label(title);
        }
        harness.get_by_label("Modifica").click();
        harness.run();
        harness.step();
        // The menu opens on the frame after the click.
        harness.get_by_label_contains("Impostazioni…").click();
        harness.run();
        assert!(
            harness.state().show_settings,
            "the menu item opened Settings"
        );
    }

    #[test]
    fn the_about_window_says_what_the_app_is() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.show_about = true;
                app
            });
        harness.run();
        harness.get_by_label_contains("MIT");
        harness.get_by_label_contains(env!("CARGO_PKG_VERSION"));
        harness.get_by_label("Chiudi").click();
        harness.run();
        assert!(!harness.state().show_about);
    }

    #[test]
    fn menu_labels_are_in_english_too() {
        let _lang = lang_guard();
        set_lang(Lang::En);
        assert_eq!(MenuAction::Scan.label(), "Scan Library");
        assert_eq!(MenuAction::Settings.label(), "Settings…");
        for action in menu::ALL_ACTIONS {
            assert!(!action.label().is_empty(), "{action:?}");
        }
        let titles: Vec<&str> = menu::menus(false).iter().map(|menu| menu.title).collect();
        assert_eq!(titles, vec!["File", "Edit", "View", "Window", "Help"]);
    }

    #[test]
    fn a_very_small_window_scrolls_instead_of_cutting_the_table() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(900.0, 460.0))
            .build_eframe(demo_app);
        harness.run();
        harness.snapshot("main-window-tiny");
    }

    #[test]
    fn the_first_run_asks_before_any_update_check() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.settings.check_updates = false;
                app.settings.update_choice_made = false;
                app.show_update_prompt = true;
                app
            });
        harness.run();
        harness.get_by_label("Aggiornamenti automatici?");
        harness.get_by_label_contains("Finché non rispondi sì");
        assert!(
            harness.state().update_check.is_none(),
            "no request before the answer"
        );
        assert!(
            !harness.state().menu_state().idle,
            "the menu waits for the answer"
        );
        harness.get_by_label("No, grazie").click();
        harness.run();
        let app = harness.state();
        assert!(!app.show_update_prompt);
        assert!(app.settings.update_choice_made);
        assert!(!app.settings.check_updates);
        assert!(app.update_check.is_none(), "still no request after No");
    }

    #[test]
    fn the_menu_bar_hover_is_quiet() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.in_window_menu = true;
                app
            });
        harness.run();
        harness.get_by_label("Modifica").hover();
        harness.run();
        harness.step();
        harness.snapshot("menu-bar-hover");
    }

    fn running_session(
        app: &mut FotogrammaApp,
        videos: Vec<VideoInfo>,
    ) -> std::sync::mpsc::Sender<crate::worker::ConversionMessage> {
        let (sender, receiver) = std::sync::mpsc::channel();
        let plan = ConversionPlan {
            videos,
            profile: fotogramma_core::conversion::profile("balanced").unwrap(),
            destination_dir: PathBuf::from("/Filmati/Fotogramma-H265"),
            container: "mkv".to_string(),
            replace_original: false,
            options: crate::worker::ConversionOptions::default(),
            after: PowerAction::Nothing,
        };
        app.session = Some(ConversionSession::new(
            plan,
            receiver,
            Arc::new(AtomicBool::new(false)),
        ));
        sender
    }

    #[test]
    fn a_minimized_conversion_leaves_the_app_usable_and_queues_more() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        let both: Vec<VideoInfo> = app
            .videos
            .iter()
            .filter(|video| video.category == fotogramma_core::scanner::Category::Convert)
            .cloned()
            .collect();
        assert_eq!(both.len(), 2);
        let _sender = running_session(app, vec![both[0].clone()]);
        assert!(!app.menu_state().idle, "the progress window is a modal");
        app.session_minimized = true;
        assert!(app.menu_state().idle, "minimized: the menu works again");
        assert!(
            !app.menu_state().enabled(MenuAction::Scan),
            "but no scan under a conversion"
        );

        // The video that is running cannot be chosen again; the other can.
        app.selected.insert(both[0].path.clone());
        app.selected.insert(both[1].path.clone());
        let chosen = app.selected_videos();
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].path, both[1].path);
        app.handle_menu_action(MenuAction::ConvertSelected, &ctx);
        assert!(app
            .conversion_dialog
            .as_ref()
            .is_some_and(|dialog| dialog.adds_to_queue));
        harness.run();
        let app = harness.state_mut();
        app.conversion_dialog = None;
        app.waiting.push(ConversionPlan {
            videos: vec![both[1].clone()],
            profile: fotogramma_core::conversion::profile("balanced").unwrap(),
            destination_dir: PathBuf::from("/x"),
            container: "mkv".to_string(),
            replace_original: false,
            options: crate::worker::ConversionOptions::default(),
            after: PowerAction::Nothing,
        });
        if let Some(session) = app.session.as_mut() {
            session.percent = 37;
            session.label = "1 / 1 · Gita al lago.mp4 · 37% · circa 41 min rimasti".to_string();
        }
        harness.run();
        harness.snapshot("conversion-background");
    }

    #[test]
    fn stopping_the_running_batch_drops_the_waiting_ones() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let ctx = harness.ctx.clone();
        let app = harness.state_mut();
        let sender = running_session(app, Vec::new());
        app.waiting.push(ConversionPlan {
            videos: Vec::new(),
            profile: fotogramma_core::conversion::profile("balanced").unwrap(),
            destination_dir: PathBuf::from("/x"),
            container: "mkv".to_string(),
            replace_original: false,
            options: crate::worker::ConversionOptions::default(),
            after: PowerAction::Nothing,
        });
        app.session
            .as_ref()
            .unwrap()
            .cancel
            .store(true, Ordering::Relaxed);
        sender
            .send(crate::worker::ConversionMessage::Completed(Vec::new()))
            .unwrap();
        app.session.as_mut().unwrap().poll();
        app.advance_queue(&ctx);
        assert!(app.waiting.is_empty(), "Stop stops the whole queue");
    }

    #[test]
    fn closing_during_a_conversion_mentions_the_queue() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let app = harness.state_mut();
        let _sender = running_session(app, Vec::new());
        for _ in 0..2 {
            app.waiting.push(ConversionPlan {
                videos: Vec::new(),
                profile: fotogramma_core::conversion::profile("balanced").unwrap(),
                destination_dir: PathBuf::from("/x"),
                container: "mkv".to_string(),
                replace_original: false,
                options: crate::worker::ConversionOptions::default(),
                after: PowerAction::Nothing,
            });
        }
        app.close_confirm = true;
        harness.run();
        harness.get_by_label("Vuoi interrompere la conversione e chiudere Fotogramma?");
        harness.get_by_label_contains("Anche i 2 gruppi di video in coda andranno persi");
    }

    #[test]
    fn a_manual_update_check_answers_with_a_card() {
        use egui_kittest::kittest::Queryable;
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(|cc| {
                let mut app = demo_app(cc);
                app.update_notice = Some(dialogs::UpdateNotice::UpToDate);
                app
            });
        harness.run();
        harness.get_by_label("Sei aggiornato");
        harness.get_by_label_contains(env!("CARGO_PKG_VERSION"));
        harness.snapshot("update-up-to-date");
        harness.get_by_label("Perfetto").click();
        harness.run();
        assert!(harness.state().update_notice.is_none());
    }

    #[test]
    fn a_dismissed_check_does_not_come_back_with_its_answer() {
        let _lang = lang_guard();
        let mut harness = egui_kittest::Harness::builder()
            .with_size(egui::vec2(1050.0, 680.0))
            .build_eframe(demo_app);
        harness.run();
        let app = harness.state_mut();
        let (sender, receiver) = std::sync::mpsc::channel();
        app.update_check = Some(receiver);
        app.update_manual = true;
        app.update_notice = None; // the user closed the card
        sender.send(CheckMessage::UpToDate).unwrap();
        app.poll_update();
        assert!(app.update_notice.is_none());
    }
}
