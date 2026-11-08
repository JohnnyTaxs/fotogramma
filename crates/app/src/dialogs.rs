//! Conversion setup, conversion progress and detail dialogs.

use eframe::egui::{self, RichText};
use fotogramma_core::conversion::{
    allocate_output_path, allocate_partial_path, build_ffmpeg_args, builtin_profiles,
    custom_profile, profile as builtin_profile, AudioMode, ConversionProfile, PRESETS,
};
use fotogramma_core::formatting::{codec_label, format_bytes, format_duration};
use fotogramma_core::scanner::VideoInfo;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use crate::theme;
use crate::worker::{ConversionMessage, ConversionResult};

const AUDIO_OPTIONS: [(&str, Option<u32>); 4] = [
    ("Originale · nessuna ricompressione", None),
    ("AAC 128 kbps", Some(128)),
    ("AAC 160 kbps", Some(160)),
    ("AAC 256 kbps", Some(256)),
];

pub struct ConversionDialog {
    pub videos: Vec<VideoInfo>,
    pub profile_key: String,
    pub crf: u8,
    pub preset: String,
    pub pixel: String,
    pub tune: String,
    pub aq: Option<u8>,
    pub audio_index: usize,
    pub container: String,
    pub destination: String,
    pub replace_original: bool,
}

pub struct ConversionPlan {
    pub videos: Vec<VideoInfo>,
    pub profile: ConversionProfile,
    pub destination_dir: PathBuf,
    pub container: String,
    pub replace_original: bool,
}

pub enum DialogOutcome {
    None,
    Cancel,
    Start(ConversionPlan),
}

impl ConversionDialog {
    pub fn new(videos: Vec<VideoInfo>) -> Self {
        let destination = videos
            .first()
            .and_then(|video| video.path.parent())
            .map(|parent| parent.join("Fotogramma-H265"))
            .unwrap_or_else(|| PathBuf::from("Fotogramma-H265"));
        let balanced = builtin_profile("balanced").expect("balanced profile");
        Self {
            videos,
            profile_key: "balanced".to_string(),
            crf: balanced.crf,
            preset: balanced.preset,
            pixel: balanced.pixel_format,
            tune: String::new(),
            aq: balanced.aq_mode,
            audio_index: 0,
            container: "mkv".to_string(),
            destination: destination.to_string_lossy().into_owned(),
            replace_original: false,
        }
    }

    fn apply_profile(&mut self) {
        if let Some(profile) = builtin_profile(&self.profile_key) {
            self.crf = profile.crf;
            self.preset = profile.preset;
            self.pixel = profile.pixel_format;
            self.tune = profile.tune.unwrap_or_default();
            self.aq = profile.aq_mode;
        }
    }

    pub fn current_profile(&self) -> ConversionProfile {
        if self.profile_key != "custom" {
            if let Some(profile) = builtin_profile(&self.profile_key) {
                return profile;
            }
        }
        let audio = AUDIO_OPTIONS[self.audio_index.min(AUDIO_OPTIONS.len() - 1)];
        custom_profile(
            self.crf,
            &self.preset,
            &self.pixel,
            Some(self.tune.as_str()),
            self.aq,
            if audio.1.is_some() {
                AudioMode::Aac
            } else {
                AudioMode::Copy
            },
            audio.1.unwrap_or(160),
        )
    }

    fn plan(&self) -> Result<ConversionPlan, String> {
        let Some(first) = self.videos.first() else {
            return Err("Nessun video selezionato.".to_string());
        };
        let profile = self.current_profile();
        let (container, destination_dir) = if self.replace_original {
            let extension = first
                .path
                .extension()
                .map(|value| value.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if extension != "mkv" && extension != "mp4" {
                return Err(
                    "La sostituzione dell’originale è disponibile solo per MKV e MP4.".to_string(),
                );
            }
            (
                extension,
                first
                    .path
                    .parent()
                    .map(|parent| parent.to_path_buf())
                    .unwrap_or_else(|| PathBuf::from(".")),
            )
        } else {
            if self.destination.trim().is_empty() {
                return Err("Scegli una cartella di destinazione.".to_string());
            }
            (
                self.container.clone(),
                PathBuf::from(self.destination.trim()),
            )
        };
        Ok(ConversionPlan {
            videos: self.videos.clone(),
            profile,
            destination_dir,
            container,
            replace_original: self.replace_original,
        })
    }

    fn preview(&self) -> String {
        let Some(first) = self.videos.first() else {
            return "Nessun video selezionato.".to_string();
        };
        let destination = if self.replace_original {
            let extension = first
                .path
                .extension()
                .map(|value| value.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if extension != "mkv" && extension != "mp4" {
                return "La sostituzione dell’originale è disponibile solo per MKV e MP4."
                    .to_string();
            }
            allocate_partial_path(&first.path).unwrap_or_else(|_| first.path.clone())
        } else {
            if self.destination.trim().is_empty() {
                return "Scegli una cartella di destinazione.".to_string();
            }
            allocate_output_path(
                &first.path,
                Path::new(self.destination.trim()),
                &self.container,
            )
            .unwrap_or_else(|error| PathBuf::from(error))
        };
        let profile = self.current_profile();
        let args = build_ffmpeg_args(&profile, &first.path, &destination);
        let mut command = vec!["ffmpeg".to_string()];
        command.extend(args);
        command
            .iter()
            .map(|argument| quote(argument))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn show(&mut self, ctx: &egui::Context) -> DialogOutcome {
        let mut outcome = DialogOutcome::None;
        let screen_height = ctx.content_rect().height();
        let modal_id = egui::Id::unique("conversion-dialog");
        let modal_area = egui::Modal::default_area(modal_id).default_size(egui::vec2(
            820.0,
            (screen_height - 120.0).clamp(480.0, 680.0),
        ));
        let modal = egui::Modal::new(modal_id)
            .area(modal_area)
            .frame(modal_frame())
            .show(ctx, |ui| {
                ui.set_min_width(720.0);
                ui.set_max_width(760.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(
                    RichText::new("Converti in H.265")
                        .size(theme::PAGE_TITLE_SIZE)
                        .strong(),
                );
                ui.label(
                    RichText::new(if self.replace_original {
                        format!(
                            "{} file. Gli originali saranno sostituiti solo dopo test completi.",
                            self.videos.len()
                        )
                    } else {
                        format!(
                            "{} file selezionati. Gli originali restano invariati.",
                            self.videos.len()
                        )
                    })
                    .strong(),
                );
                ui.add_space(6.0);

                let max_scroll = (screen_height - 220.0).max(280.0);
                egui::ScrollArea::vertical()
                    .max_height(max_scroll)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                    let custom = self.profile_key == "custom";
                    let mut selected_profile = self.profile_key.clone();
                    egui::Grid::new("conversion-form")
                        .num_columns(2)
                        .spacing([18.0, 4.0])
                        .show(ui, |ui| {
                            let profile_label = ui.label("Profilo");
                            let profiles = builtin_profiles();
                            let selected_label = profiles
                                .iter()
                                .find(|profile| profile.key == self.profile_key)
                                .map(|profile| profile.label.clone())
                                .unwrap_or_else(|| "Personalizzato".to_string());
                            egui::ComboBox::from_id_salt("profile-combo")
                                .width(360.0)
                                .selected_text(selected_label)
                                .show_ui(ui, |ui| {
                                    for profile in &profiles {
                                        ui.selectable_value(
                                            &mut selected_profile,
                                            profile.key.clone(),
                                            profile.label.clone(),
                                        );
                                    }
                                    ui.selectable_value(
                                        &mut selected_profile,
                                        "custom".to_string(),
                                        "Personalizzato",
                                    );
                                })
                                .response
                                .labelled_by(profile_label.id);
                            ui.end_row();

                            let crf_label = ui.label("CRF · qualità");
                            ui.add_enabled_ui(custom, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().slider_width = 300.0;
                                    ui.add(
                                        egui::Slider::new(&mut self.crf, 16..=35).show_value(false),
                                    )
                                    .labelled_by(crf_label.id);
                                    ui.add(
                                        egui::DragValue::new(&mut self.crf)
                                            .range(16..=35)
                                            .speed(0.2),
                                    )
                                    .labelled_by(crf_label.id);
                                });
                            });
                            ui.end_row();

                            let preset_label = ui.label("Preset · velocità");
                            ui.add_enabled_ui(custom, |ui| {
                                egui::ComboBox::from_id_salt("preset-combo")
                                    .width(360.0)
                                    .selected_text(&self.preset)
                                    .show_ui(ui, |ui| {
                                        for preset in PRESETS {
                                            ui.selectable_value(
                                                &mut self.preset,
                                                preset.to_string(),
                                                preset,
                                            );
                                        }
                                    })
                                    .response
                                    .labelled_by(preset_label.id);
                            });
                            ui.end_row();

                            let pixel_field = ui.label("Formato pixel");
                            ui.add_enabled_ui(custom, |ui| {
                                egui::ComboBox::from_id_salt("pixel-combo")
                                    .width(360.0)
                                    .selected_text(pixel_label(&self.pixel))
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.pixel,
                                            "yuv420p10le".to_string(),
                                            "10-bit · banding ridotto",
                                        );
                                        ui.selectable_value(
                                            &mut self.pixel,
                                            "yuv420p".to_string(),
                                            "8-bit · massima compatibilità",
                                        );
                                    })
                                    .response
                                    .labelled_by(pixel_field.id);
                            });
                            ui.end_row();

                            let tune_field = ui.label("Ottimizzazione contenuto");
                            ui.add_enabled_ui(custom, |ui| {
                                egui::ComboBox::from_id_salt("tune-combo")
                                    .width(360.0)
                                    .selected_text(tune_label(&self.tune))
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.tune,
                                            String::new(),
                                            "Nessuno",
                                        );
                                        ui.selectable_value(
                                            &mut self.tune,
                                            "animation".to_string(),
                                            "Animation",
                                        );
                                        ui.selectable_value(
                                            &mut self.tune,
                                            "film".to_string(),
                                            "Film",
                                        );
                                        ui.selectable_value(
                                            &mut self.tune,
                                            "grain".to_string(),
                                            "Grain",
                                        );
                                    })
                                    .response
                                    .labelled_by(tune_field.id);
                            });
                            ui.end_row();

                            let aq_label = ui.label("Quantizzazione adattiva");
                            ui.add_enabled_ui(custom, |ui| {
                                egui::ComboBox::from_id_salt("aq-combo")
                                    .width(360.0)
                                    .selected_text(match self.aq {
                                        None => "Automatico".to_string(),
                                        Some(value) => format!("AQ mode {value}"),
                                    })
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(&mut self.aq, None, "Automatico");
                                        for value in 0..=3u8 {
                                            ui.selectable_value(
                                                &mut self.aq,
                                                Some(value),
                                                format!("AQ mode {value}"),
                                            );
                                        }
                                    })
                                    .response
                                    .labelled_by(aq_label.id);
                            });
                            ui.end_row();

                            let audio_label = ui.label("Audio");
                            egui::ComboBox::from_id_salt("audio-combo")
                                .width(360.0)
                                .selected_text(AUDIO_OPTIONS[self.audio_index].0)
                                .show_ui(ui, |ui| {
                                    for (index, (label, _)) in AUDIO_OPTIONS.iter().enumerate() {
                                        ui.selectable_value(&mut self.audio_index, index, *label);
                                    }
                                })
                                .response
                                .labelled_by(audio_label.id);
                            ui.end_row();

                            let container_field = ui.label("Contenitore");
                            ui.add_enabled_ui(!self.replace_original, |ui| {
                                egui::ComboBox::from_id_salt("container-combo")
                                    .width(360.0)
                                    .selected_text(container_label(&self.container))
                                    .show_ui(ui, |ui| {
                                        ui.selectable_value(
                                            &mut self.container,
                                            "mkv".to_string(),
                                            "MKV · consigliato",
                                        );
                                        ui.selectable_value(
                                            &mut self.container,
                                            "mp4".to_string(),
                                            "MP4 · faststart",
                                        );
                                    })
                                    .response
                                    .labelled_by(container_field.id);
                            });
                            ui.end_row();
                        });

                    if selected_profile != self.profile_key {
                        self.profile_key = selected_profile;
                        self.apply_profile();
                    }

                    let description = if self.profile_key == "custom" {
                        "Configura manualmente i parametri video. Audio e contenitore restano modificabili in ogni profilo.".to_string()
                    } else {
                        builtin_profile(&self.profile_key)
                            .map(|profile| profile.description)
                            .unwrap_or_default()
                    };
                    ui.label(RichText::new(description).small().color(theme::MUTED));
                    ui.add_space(4.0);

                    ui.group(|ui| {
                        ui.set_min_width(ui.available_width());
                        let destination_field = ui.label(
                            RichText::new(if self.replace_original {
                                "Sostituzione solo dopo test completi"
                            } else {
                                "Destinazione · mai sovrascrivere"
                            })
                            .strong(),
                        );
                        ui.horizontal(|ui| {
                            ui.add_enabled_ui(!self.replace_original, |ui| {
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.destination)
                                        .desired_width(f32::INFINITY),
                                )
                                .labelled_by(destination_field.id);
                            });
                            if ui
                                .add_enabled(!self.replace_original, theme::secondary_button("Scegli…"))
                                .clicked()
                            {
                                if let Some(selected) = rfd::FileDialog::new()
                                    .set_directory(destination_start(&self.destination))
                                    .pick_folder()
                                {
                                    self.destination = selected.to_string_lossy().into_owned();
                                }
                            }
                        });
                    });

                    ui.checkbox(
                        &mut self.replace_original,
                        "Sostituisci gli originali solo dopo verifica completa",
                    )
                    .on_hover_text(
                        "Scrive prima un file temporaneo, esegue tutti i test e solo dopo sostituisce l’originale con backup di sicurezza",
                    );
                    if self.replace_original {
                        if let Some(first) = self.videos.first() {
                            let extension = first
                                .path
                                .extension()
                                .map(|value| value.to_string_lossy().to_lowercase())
                                .unwrap_or_default();
                            if extension == "mkv" || extension == "mp4" {
                                self.container = extension;
                            }
                        }
                    }

                    ui.add_space(4.0);
                    ui.label(RichText::new("Anteprima del comando FFmpeg").strong());
                    let mut preview = self.preview();
                    ui.add(
                        egui::TextEdit::multiline(&mut preview)
                            .font(theme::mono(11.0))
                            .desired_width(f32::INFINITY)
                            .desired_rows(3)
                            .interactive(false),
                    );

                    });

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if let Err(error) = self.plan() {
                        ui.label(RichText::new(error).color(theme::ERROR));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_start = self.plan().is_ok();
                        if ui
                            .add_enabled(can_start, theme::primary_button("Avvia conversione"))
                            .clicked()
                        {
                            if let Ok(plan) = self.plan() {
                                outcome = DialogOutcome::Start(plan);
                            }
                        }
                        if ui.add(theme::secondary_button("Annulla")).clicked() {
                            outcome = DialogOutcome::Cancel;
                        }
                    });
                });
            });

        if modal.should_close() {
            if matches!(outcome, DialogOutcome::None) {
                outcome = DialogOutcome::Cancel;
            }
        }
        outcome
    }
}

fn destination_start(destination: &str) -> PathBuf {
    let path = PathBuf::from(destination);
    if path.is_dir() {
        path
    } else {
        path.parent()
            .map(|parent| parent.to_path_buf())
            .filter(|parent| parent.is_dir())
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

fn pixel_label(pixel: &str) -> &'static str {
    if pixel == "yuv420p" {
        "8-bit · massima compatibilità"
    } else {
        "10-bit · banding ridotto"
    }
}

fn tune_label(tune: &str) -> &'static str {
    match tune {
        "animation" => "Animation",
        "film" => "Film",
        "grain" => "Grain",
        _ => "Nessuno",
    }
}

fn container_label(container: &str) -> &'static str {
    if container == "mp4" {
        "MP4 · faststart"
    } else {
        "MKV · consigliato"
    }
}

fn quote(argument: &str) -> String {
    if !argument.is_empty()
        && argument
            .chars()
            .all(|character| character.is_alphanumeric() || "._-/:=+".contains(character))
    {
        return argument.to_string();
    }
    format!("'{}'", argument.replace('\'', "'\\''"))
}

fn modal_frame() -> egui::Frame {
    egui::Frame::NONE
        .fill(theme::BG)
        .stroke(egui::Stroke::new(1.0, theme::LINE))
        .corner_radius(egui::CornerRadius::same(14))
        .inner_margin(egui::Margin::same(16))
}

pub struct ConversionSession {
    pub receiver: Receiver<ConversionMessage>,
    pub destination_dir: PathBuf,
    pub replace_original: bool,
    pub cancel: Arc<AtomicBool>,
    pub label: String,
    pub percent: u8,
    pub log: Vec<String>,
    pub results: Vec<ConversionResult>,
    pub finished: bool,
    pub failed: bool,
}

impl ConversionSession {
    pub fn new(
        plan: ConversionPlan,
        receiver: Receiver<ConversionMessage>,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            receiver,
            destination_dir: plan.destination_dir,
            replace_original: plan.replace_original,
            cancel,
            label: "Preparazione…".to_string(),
            percent: 0,
            log: Vec::new(),
            results: Vec::new(),
            finished: false,
            failed: false,
        }
    }

    pub fn poll(&mut self) {
        while let Ok(message) = self.receiver.try_recv() {
            match message {
                ConversionMessage::FileStarted {
                    index,
                    total,
                    source,
                    destination,
                } => {
                    self.label = format!(
                        "{index} / {total} · {}",
                        source
                            .file_name()
                            .map(|value| value.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    );
                    self.log.push(format!(
                        "Avvio: {}\nOutput: {}",
                        source.display(),
                        destination.display()
                    ));
                    self.percent = 0;
                }
                ConversionMessage::Progress {
                    index,
                    total,
                    percent,
                    current_file,
                } => {
                    self.percent = percent;
                    self.label = format!(
                        "{index} / {total} · {} · {percent}%",
                        current_file
                            .file_name()
                            .map(|value| value.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    );
                }
                ConversionMessage::VerificationStarted {
                    index,
                    current_file,
                } => {
                    self.label = format!(
                        "Verifica completa · {index} · {}",
                        current_file
                            .file_name()
                            .map(|value| value.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    );
                    self.log
                        .push("Verifica completa prima della sostituzione…".to_string());
                }
                ConversionMessage::FileCompleted(result) => {
                    let result = *result;
                    if result.cancelled {
                        self.log
                            .push(format!("Interrotto: {}", result.source.display()));
                    } else if result.verified {
                        self.log.push(format!(
                            "Verifica superata ✓ · originale sostituito: {}",
                            result.destination.display()
                        ));
                    } else if result.success {
                        self.log
                            .push(format!("Completato ✓ · {}", result.destination.display()));
                    } else {
                        self.log.push(format!(
                            "Errore su {}: {}",
                            result.destination.display(),
                            result.message
                        ));
                    }
                    self.results.push(result);
                }
                ConversionMessage::Completed(results) => {
                    self.finished = true;
                    self.results = results;
                    let successes = self.results.iter().filter(|result| result.success).count();
                    let errors = self.results.len().saturating_sub(successes);
                    let replacements = self.results.iter().filter(|result| result.replaced).count();
                    let mut summary =
                        format!("Conversione terminata: {successes} riuscite, {errors} errori");
                    if replacements > 0 {
                        summary.push_str(&format!(
                            " · {replacements} originali sostituiti dopo verifica"
                        ));
                    }
                    self.label = summary;
                    self.percent = 100;
                }
                ConversionMessage::Failed(message) => {
                    self.log.push(message);
                    self.label = "Conversione non riuscita".to_string();
                    self.finished = true;
                    self.failed = true;
                }
            }
        }
    }

    pub fn cancel(&self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn successes(&self) -> usize {
        self.results.iter().filter(|result| result.success).count()
    }

    pub fn is_running(&self) -> bool {
        !self.finished
    }
}

pub enum ProgressAction {
    None,
    Cancel,
    Close,
    OpenFolder,
}

pub fn show_progress(ctx: &egui::Context, session: &mut ConversionSession) -> ProgressAction {
    let mut action = ProgressAction::None;
    let modal = egui::Modal::new(egui::Id::unique("conversion-progress"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(680.0);
            let heading = if session.failed {
                "Conversione non riuscita"
            } else if session.finished {
                "Conversione completata"
            } else {
                "Conversione in corso"
            };
            ui.label(RichText::new(heading).size(theme::PAGE_TITLE_SIZE).strong());
            ui.label(&session.label);
            ui.add(
                egui::ProgressBar::new(f32::from(session.percent) / 100.0)
                    .desired_height(10.0)
                    .fill(theme::CORAL),
            );
            if session.replace_original && !session.finished {
                ui.label(
                    RichText::new("Gli originali vengono toccati solo dopo la verifica completa.")
                        .small()
                        .color(theme::MUTED),
                );
            }
            egui::ScrollArea::vertical()
                .max_height(260.0)
                .auto_shrink([false, true])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    for line in &session.log {
                        ui.label(RichText::new(line).font(theme::mono(10.0)));
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if session.finished && session.successes() > 0 {
                    if ui.add(theme::secondary_button("Apri cartella")).clicked() {
                        action = ProgressAction::OpenFolder;
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if session.finished {
                        if ui.add(theme::primary_button("Chiudi")).clicked() {
                            action = ProgressAction::Close;
                        }
                    } else if ui.add(theme::secondary_button("Interrompi")).clicked() {
                        session.cancel();
                        action = ProgressAction::Cancel;
                    }
                });
            });
        });

    if modal.should_close() && session.finished {
        action = ProgressAction::Close;
    }
    action
}

pub fn show_details(ctx: &egui::Context, video: &VideoInfo) -> bool {
    let mut closed = false;
    let modal = egui::Modal::new(egui::Id::unique("video-details"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(560.0);
            ui.label(RichText::new(&video.name).size(21.0).strong());
            ui.add_space(6.0);
            egui::Grid::new("details-grid")
                .num_columns(2)
                .spacing([16.0, 6.0])
                .show(ui, |ui| {
                    ui.label("Percorso");
                    ui.label(
                        RichText::new(video.path.display().to_string()).font(theme::mono(10.0)),
                    );
                    ui.end_row();
                    ui.label("Codec");
                    ui.label(codec_label(video.codec.as_deref()));
                    ui.end_row();
                    ui.label("Durata");
                    ui.label(format_duration(video.duration));
                    ui.end_row();
                    ui.label("Risoluzione");
                    ui.label(format!(
                        "{} × {}",
                        video.width.unwrap_or(0),
                        video.height.unwrap_or(0)
                    ));
                    ui.end_row();
                    ui.label("Dimensione");
                    ui.label(format_bytes(video.size));
                    ui.end_row();
                    ui.label("Stato");
                    ui.label(video.status_label());
                    ui.end_row();
                });
            if let Some(error) = &video.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(theme::ERROR));
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.add(theme::primary_button("Chiudi")).clicked() {
                    closed = true;
                }
            });
        });
    closed || modal.should_close()
}

pub enum CloseChoice {
    StopAndClose,
    KeepOpen,
}

pub fn show_close_confirm(ctx: &egui::Context) -> Option<CloseChoice> {
    let mut choice = None;
    let modal = egui::Modal::new(egui::Id::unique("close-confirm"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(480.0);
            ui.label(RichText::new("Analisi in corso").size(21.0).strong());
            ui.label("Vuoi interrompere l’analisi e chiudere Fotogramma?");
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new("Interrompi e chiudi").color(theme::CORAL_INK),
                        )
                        .fill(theme::CORAL),
                    )
                    .clicked()
                {
                    choice = Some(CloseChoice::StopAndClose);
                }
                if ui.add(theme::secondary_button("Continua")).clicked() {
                    choice = Some(CloseChoice::KeepOpen);
                }
            });
        });
    if modal.should_close() && choice.is_none() {
        choice = Some(CloseChoice::KeepOpen);
    }
    choice
}

impl ConversionSession {
    pub fn destination_dir(&self) -> &Path {
        &self.destination_dir
    }
}
