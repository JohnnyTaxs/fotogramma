//! Conversion setup, conversion progress and detail dialogs.

use eframe::egui::{self, RichText};
use fotogramma_core::conversion::{
    allocate_output_path, allocate_partial_path, build_ffmpeg_args, builtin_profiles,
    custom_profile, detect_selection, profile as builtin_profile, AudioMode, ConversionProfile,
    MediaKind, PRESETS,
};
use fotogramma_core::formatting::{codec_label, format_bytes, format_duration, SpaceChange};
use fotogramma_core::scanner::VideoInfo;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

use crate::platform::PowerAction;
use crate::settings::{SavedPreset, Settings, KNOWN_CODECS};
use crate::theme;
use crate::worker::{ConversionMessage, ConversionOptions, ConversionResult, InstallMessage};
use fotogramma_core::i18n::tr_str;
use fotogramma_core::update::{InstallTarget, Release};

fn audio_options() -> [(&'static str, Option<u32>); 4] {
    [
        (
            tr_str(
                "Originale · nessuna ricompressione",
                "Original · no re-encoding",
            ),
            None,
        ),
        ("AAC 128 kbps", Some(128)),
        ("AAC 160 kbps", Some(160)),
        ("AAC 256 kbps", Some(256)),
    ]
}

pub struct ConversionDialog {
    pub videos: Vec<VideoInfo>,
    pub profile_key: String,
    pub crf: u8,
    pub preset: String,
    pub pixel: String,
    pub tune: String,
    /// `auto`, `anime` or `live`.
    pub content: String,
    /// What the paths of the selection suggest.
    pub detected: Option<MediaKind>,
    pub aq: Option<u8>,
    pub audio_index: usize,
    pub container: String,
    pub destination: String,
    pub replace_original: bool,
    pub options: ConversionOptions,
    /// `list`, `largest` or `smallest`: which files go first.
    pub order: String,
    /// The user's saved presets, and whether they changed in this dialog.
    pub presets: Vec<SavedPreset>,
    pub presets_dirty: bool,
    /// The name typed to save the current settings.
    pub preset_name: String,
    /// The saved preset the settings were loaded from, if any.
    pub loaded_preset: Option<String>,
    /// What to do with the computer when the queue ends.
    pub after: PowerAction,
    /// A conversion is already running: this one waits behind it.
    pub adds_to_queue: bool,
}

pub struct ConversionPlan {
    pub videos: Vec<VideoInfo>,
    pub profile: ConversionProfile,
    pub destination_dir: PathBuf,
    pub container: String,
    pub replace_original: bool,
    pub options: ConversionOptions,
    pub after: PowerAction,
}

pub enum DialogOutcome {
    None,
    Cancel,
    Start(Box<ConversionPlan>),
}

impl ConversionDialog {
    pub fn new(videos: Vec<VideoInfo>, defaults: &Settings) -> Self {
        let destination = videos
            .first()
            .map(|video| {
                fotogramma_core::conversion::output_dir_for(&video.path, &defaults.output_dir)
            })
            .unwrap_or_else(|| PathBuf::from(defaults.output_dir.trim()));
        let detected = detect_selection(videos.iter().map(|video| video.path.as_path()));
        let balanced = builtin_profile("balanced").expect("balanced profile");
        let mut dialog = Self {
            videos,
            profile_key: "balanced".to_string(),
            crf: balanced.crf,
            preset: balanced.preset,
            pixel: balanced.pixel_format,
            tune: String::new(),
            content: defaults.content_type.clone(),
            detected,
            aq: balanced.aq_mode,
            audio_index: 0,
            container: if defaults.default_container == "mp4" {
                "mp4".to_string()
            } else {
                "mkv".to_string()
            },
            destination: destination.to_string_lossy().into_owned(),
            replace_original: defaults.replace_original,
            options: ConversionOptions {
                min_saving_percent: defaults.min_saving_percent,
                prevent_sleep: defaults.prevent_sleep,
                ..ConversionOptions::default()
            },
            order: "list".to_string(),
            presets: defaults.presets.clone(),
            presets_dirty: false,
            preset_name: String::new(),
            loaded_preset: None,
            after: PowerAction::Nothing,
            adds_to_queue: false,
        };
        // A saved profile that no longer exists falls back to "balanced".
        if defaults.default_profile != "custom"
            && builtin_profile(&defaults.default_profile).is_some()
        {
            dialog.profile_key = defaults.default_profile.clone();
        }
        dialog.apply_profile();
        dialog
    }

    /// The kind the encoder settings follow: the user's choice, or what the
    /// paths suggest when the choice is "auto".
    pub fn effective_kind(&self) -> Option<MediaKind> {
        MediaKind::from_setting(&self.content).or(if self.content == "auto" {
            self.detected
        } else {
            None
        })
    }

    /// Save the settings of the custom profile under `name`, replacing a
    /// preset of the same name. A blank name saves nothing.
    pub fn save_preset(&mut self, name: &str) -> bool {
        let name: String = name.trim().chars().take(40).collect();
        if name.is_empty() {
            return false;
        }
        let preset = SavedPreset {
            name: name.clone(),
            crf: self.crf,
            preset: self.preset.clone(),
            pixel: self.pixel.clone(),
            tune: self.tune.clone(),
            aq: self.aq,
            audio_index: self.audio_index,
            container: self.container.clone(),
        };
        match self
            .presets
            .iter_mut()
            .find(|existing| existing.name == name)
        {
            Some(existing) => *existing = preset,
            None => self.presets.push(preset),
        }
        self.presets_dirty = true;
        self.loaded_preset = Some(name);
        true
    }

    /// Put a saved preset into the custom settings.
    pub fn load_preset(&mut self, name: &str) -> bool {
        let Some(preset) = self
            .presets
            .iter()
            .find(|preset| preset.name == name)
            .cloned()
        else {
            return false;
        };
        self.profile_key = "custom".to_string();
        self.crf = preset.crf;
        self.preset = preset.preset;
        self.pixel = preset.pixel;
        self.tune = preset.tune;
        self.aq = preset.aq;
        self.audio_index = preset.audio_index.min(audio_options().len() - 1);
        self.container = preset.container;
        self.loaded_preset = Some(preset.name);
        true
    }

    pub fn delete_preset(&mut self, name: &str) {
        let before = self.presets.len();
        self.presets.retain(|preset| preset.name != name);
        if self.presets.len() != before {
            self.presets_dirty = true;
        }
        if self.loaded_preset.as_deref() == Some(name) {
            self.loaded_preset = None;
        }
    }

    fn kind_tune(&self) -> Option<String> {
        self.effective_kind()
            .and_then(MediaKind::tune)
            .map(str::to_string)
    }

    fn apply_profile(&mut self) {
        if let Some(profile) = builtin_profile(&self.profile_key) {
            self.crf = profile.crf;
            self.preset = profile.preset;
            self.pixel = profile.pixel_format;
            self.aq = profile.aq_mode;
        }
        self.tune = self.kind_tune().unwrap_or_default();
    }

    pub fn current_profile(&self) -> ConversionProfile {
        let audio = audio_options()[self.audio_index.min(audio_options().len() - 1)];
        let (audio_mode, audio_bitrate) = if audio.1.is_some() {
            (AudioMode::Aac, audio.1.unwrap_or(160))
        } else {
            (AudioMode::Copy, 160)
        };
        if self.profile_key != "custom" {
            if let Some(mut profile) = builtin_profile(&self.profile_key) {
                profile.tune = self.kind_tune();
                profile.audio_mode = audio_mode;
                profile.audio_bitrate = audio_bitrate;
                return profile;
            }
        }
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
            return Err(tr_str("Nessun video selezionato.", "No video selected.").to_string());
        };
        let profile = self.current_profile();
        let (container, destination_dir) = if self.replace_original {
            let extension = first
                .path
                .extension()
                .map(|value| value.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if extension != "mkv" && extension != "mp4" {
                return Err(tr_str(
                    "La sostituzione dell’originale è disponibile solo per MKV e MP4.",
                    "Replacing the original is only available for MKV and MP4.",
                )
                .to_string());
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
                return Err(tr_str(
                    "Scegli una cartella di destinazione.",
                    "Choose a destination folder.",
                )
                .to_string());
            }
            (
                self.container.clone(),
                PathBuf::from(self.destination.trim()),
            )
        };
        let mut videos = self.videos.clone();
        match self.order.as_str() {
            "largest" => videos.sort_by_key(|video| std::cmp::Reverse(video.size.unwrap_or(0))),
            "smallest" => videos.sort_by_key(|video| video.size.unwrap_or(0)),
            _ => {}
        }
        Ok(ConversionPlan {
            videos,
            profile,
            destination_dir,
            container,
            replace_original: self.replace_original,
            options: self.options.clone(),
            after: self.after,
        })
    }

    fn preview(&self) -> String {
        let Some(first) = self.videos.first() else {
            return tr_str("Nessun video selezionato.", "No video selected.").to_string();
        };
        let destination = if self.replace_original {
            let extension = first
                .path
                .extension()
                .map(|value| value.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if extension != "mkv" && extension != "mp4" {
                return tr_str(
                    "La sostituzione dell’originale è disponibile solo per MKV e MP4.",
                    "Replacing the original is only available for MKV and MP4.",
                )
                .to_string();
            }
            allocate_partial_path(&first.path).unwrap_or_else(|_| first.path.clone())
        } else {
            if self.destination.trim().is_empty() {
                return tr_str(
                    "Scegli una cartella di destinazione.",
                    "Choose a destination folder.",
                )
                .to_string();
            }
            allocate_output_path(
                &first.path,
                Path::new(self.destination.trim()),
                &self.container,
            )
            .unwrap_or_else(PathBuf::from)
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

    /// The scrollable body: profile, destination, replacement, and command preview.
    fn form(&mut self, ui: &mut egui::Ui) {
        let custom = self.profile_key == "custom";
        let mut selected_profile = self.profile_key.clone();
        let description = if self.profile_key == "custom" {
            tr_str("Configura manualmente i parametri video. Audio e contenitore restano modificabili in ogni profilo.", "Configure the video parameters manually. Audio and container can be changed in every profile.").to_string()
        } else {
            builtin_profile(&self.profile_key)
                .map(|profile| profile.description)
                .unwrap_or_default()
        };
        ui.label(RichText::new(description).small().color(theme::MUTED));
        profile_comparison(ui);
        ui.add_space(6.0);
        egui::Grid::new("conversion-form")
            .num_columns(2)
            .spacing([18.0, 4.0])
            .show(ui, |ui| {
                let profile_label = ui.label(tr_str("Profilo", "Profile"));
                let profiles = builtin_profiles();
                let selected_label = profiles
                    .iter()
                    .find(|profile| profile.key == self.profile_key)
                    .map(|profile| profile.label.clone())
                    .unwrap_or_else(|| tr_str("Personalizzato", "Custom").to_string());
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
                        for preset in &self.presets {
                            ui.selectable_value(
                                &mut selected_profile,
                                format!("saved:{}", preset.name),
                                format!("• {}", preset.name),
                            );
                        }
                        ui.selectable_value(
                            &mut selected_profile,
                            "custom".to_string(),
                            tr_str("Personalizzato", "Custom"),
                        );
                    })
                    .response
                    .labelled_by(profile_label.id);
                ui.end_row();

                let content_label = ui.label(tr_str("Tipo di contenuto", "Content type"));
                let before = self.content.clone();
                egui::ComboBox::from_id_salt("content-combo")
                    .width(360.0)
                    .selected_text(content_name(&self.content))
                    .show_ui(ui, |ui| {
                        for value in ["auto", "anime", "live"] {
                            ui.selectable_value(
                                &mut self.content,
                                value.to_string(),
                                content_name(value),
                            );
                        }
                    })
                    .response
                    .labelled_by(content_label.id);
                if self.content != before {
                    self.tune = self.kind_tune().unwrap_or_default();
                }
                ui.end_row();

                let crf_label = ui.label(tr_str("CRF · qualità", "CRF · quality"));
                ui.add_enabled_ui(custom, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().slider_width = 300.0;
                        ui.add(egui::Slider::new(&mut self.crf, 16..=35).show_value(false))
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

                let preset_label = ui.label(tr_str("Preset · velocità", "Preset · speed"));
                ui.add_enabled_ui(custom, |ui| {
                    egui::ComboBox::from_id_salt("preset-combo")
                        .width(360.0)
                        .selected_text(&self.preset)
                        .show_ui(ui, |ui| {
                            for preset in PRESETS {
                                ui.selectable_value(&mut self.preset, preset.to_string(), preset);
                            }
                        })
                        .response
                        .labelled_by(preset_label.id);
                });
                ui.end_row();

                let pixel_field = ui.label(tr_str("Formato pixel", "Pixel format"));
                ui.add_enabled_ui(custom, |ui| {
                    egui::ComboBox::from_id_salt("pixel-combo")
                        .width(360.0)
                        .selected_text(pixel_label(&self.pixel))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.pixel,
                                "yuv420p10le".to_string(),
                                tr_str("10-bit · banding ridotto", "10-bit · less banding"),
                            );
                            ui.selectable_value(
                                &mut self.pixel,
                                "yuv420p".to_string(),
                                tr_str(
                                    "8-bit · massima compatibilità",
                                    "8-bit · maximum compatibility",
                                ),
                            );
                        })
                        .response
                        .labelled_by(pixel_field.id);
                });
                ui.end_row();

                let tune_field = ui.label(tr_str("Ottimizzazione contenuto", "Content tuning"));
                ui.add_enabled_ui(custom, |ui| {
                    egui::ComboBox::from_id_salt("tune-combo")
                        .width(360.0)
                        .selected_text(tune_label(&self.tune))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.tune,
                                String::new(),
                                tr_str("Nessuno", "None"),
                            );
                            ui.selectable_value(
                                &mut self.tune,
                                "animation".to_string(),
                                "Animation",
                            );
                            ui.selectable_value(&mut self.tune, "grain".to_string(), "Grain");
                        })
                        .response
                        .labelled_by(tune_field.id);
                });
                ui.end_row();

                let aq_label = ui.label(tr_str("Quantizzazione adattiva", "Adaptive quantization"));
                ui.add_enabled_ui(custom, |ui| {
                    egui::ComboBox::from_id_salt("aq-combo")
                        .width(360.0)
                        .selected_text(match self.aq {
                            None => tr_str("Automatico", "Automatic").to_string(),
                            Some(value) => format!("AQ mode {value}"),
                        })
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.aq,
                                None,
                                tr_str("Automatico", "Automatic"),
                            );
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
                    .selected_text(audio_options()[self.audio_index].0)
                    .show_ui(ui, |ui| {
                        for (index, (label, _)) in audio_options().iter().enumerate() {
                            ui.selectable_value(&mut self.audio_index, index, *label);
                        }
                    })
                    .response
                    .labelled_by(audio_label.id);
                ui.end_row();

                let container_field = ui.label(tr_str("Contenitore", "Container"));
                ui.add_enabled_ui(!self.replace_original, |ui| {
                    egui::ComboBox::from_id_salt("container-combo")
                        .width(360.0)
                        .selected_text(container_label(&self.container))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(
                                &mut self.container,
                                "mkv".to_string(),
                                tr_str("MKV · consigliato", "MKV · recommended"),
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

                let saving_label = ui.label(tr_str("Soglia di risparmio", "Minimum saving"));
                ui.horizontal(|ui| {
                    ui.add(
                        egui::Slider::new(&mut self.options.min_saving_percent, 0..=50)
                            .step_by(5.0)
                            .custom_formatter(|value, _| {
                                if value == 0.0 {
                                    tr_str("nessuna", "none").to_string()
                                } else {
                                    format!("{value}%")
                                }
                            }),
                    )
                    .labelled_by(saving_label.id);
                });
                ui.end_row();

                let order_label = ui.label(tr_str("Ordine", "Order"));
                egui::ComboBox::from_id_salt("order-combo")
                    .width(360.0)
                    .selected_text(order_name(&self.order))
                    .show_ui(ui, |ui| {
                        for value in ["list", "largest", "smallest"] {
                            ui.selectable_value(
                                &mut self.order,
                                value.to_string(),
                                order_name(value),
                            );
                        }
                    })
                    .response
                    .labelled_by(order_label.id);
                ui.end_row();

                let after_label = ui.label(tr_str("Al termine", "When finished"));
                egui::ComboBox::from_id_salt("after-combo")
                    .width(360.0)
                    .selected_text(after_name(self.after))
                    .show_ui(ui, |ui| {
                        for value in [
                            PowerAction::Nothing,
                            PowerAction::Sleep,
                            PowerAction::Shutdown,
                        ] {
                            ui.selectable_value(&mut self.after, value, after_name(value));
                        }
                    })
                    .response
                    .labelled_by(after_label.id);
                ui.end_row();
            });

        if let Some(name) = selected_profile.strip_prefix("saved:") {
            self.load_preset(name);
        } else if selected_profile != self.profile_key {
            self.profile_key = selected_profile;
            self.loaded_preset = None;
            self.apply_profile();
        }

        if self.content == "auto" {
            let text = match self.detected {
                Some(kind) => fotogramma_core::tr!(
                    "Rilevato dal percorso: {}.",
                    "Detected from the path: {}.",
                    kind_name(kind)
                ),
                None => tr_str(
                    "Nessun indizio nel percorso: impostazioni standard.",
                    "No hint in the path: standard settings.",
                )
                .to_string(),
            };
            ui.label(RichText::new(text).small().color(theme::MUTED));
        }
        ui.add_space(4.0);

        if self.profile_key == "custom" {
            ui.horizontal(|ui| {
                let name_label = ui.label(tr_str("Nome del preset", "Preset name"));
                ui.add(
                    egui::TextEdit::singleline(&mut self.preset_name)
                        .hint_text(tr_str("es. Anime 1080p", "e.g. Anime 1080p"))
                        .desired_width(200.0),
                )
                .labelled_by(name_label.id);
                let name = self.preset_name.clone();
                if ui
                    .add_enabled(
                        !name.trim().is_empty(),
                        theme::secondary_button(tr_str("Salva preset", "Save preset")),
                    )
                    .clicked()
                    && self.save_preset(&name)
                {
                    self.preset_name.clear();
                }
                if let Some(loaded) = self.loaded_preset.clone() {
                    if ui
                        .add(theme::secondary_button(fotogramma_core::tr!(
                            "Elimina «{}»",
                            "Delete “{}”",
                            loaded
                        )))
                        .clicked()
                    {
                        self.delete_preset(&loaded);
                    }
                }
            });
        }

        ui.group(|ui| {
            ui.set_min_width(ui.available_width());
            let destination_field = ui.label(
                RichText::new(if self.replace_original {
                    tr_str(
                        "Sostituzione solo dopo test completi",
                        "Replacement only after full tests",
                    )
                } else {
                    tr_str(
                        "Destinazione · mai sovrascrivere",
                        "Destination · never overwrite",
                    )
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
                    .add_enabled(
                        !self.replace_original,
                        theme::secondary_button(tr_str("Scegli…", "Choose…")),
                    )
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
                        tr_str("Sostituisci gli originali solo dopo verifica completa", "Replace the originals only after full verification"),
                    )
                    .on_hover_text(
                        tr_str("Scrive prima un file temporaneo, esegue tutti i test e solo dopo sostituisce l’originale con backup di sicurezza", "Writes a temporary file first, runs every test, and only then replaces the original, keeping a safety backup"),
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
        ui.label(
            RichText::new(tr_str(
                "Anteprima del comando FFmpeg",
                "FFmpeg command preview",
            ))
            .strong(),
        );
        let mut preview = self.preview();
        ui.add(
            egui::TextEdit::multiline(&mut preview)
                .font(theme::mono(11.0))
                .desired_width(f32::INFINITY)
                .desired_rows(3)
                .interactive(false),
        );
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
                    RichText::new(tr_str("Converti in H.265", "Convert to H.265"))
                        .size(theme::PAGE_TITLE_SIZE)
                        .strong(),
                );
                ui.label(
                    RichText::new(if self.replace_original {
                        fotogramma_core::tr!(
                            "File: {}. Gli originali saranno sostituiti solo dopo test completi.",
                            "Files: {}. The originals are replaced only after full tests.",
                            self.videos.len()
                        )
                    } else {
                        fotogramma_core::tr!(
                            "File selezionati: {}. Gli originali restano invariati.",
                            "Files selected: {}. The originals stay unchanged.",
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
                    .show(ui, |ui| self.form(ui));

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if let Err(error) = self.plan() {
                        ui.label(RichText::new(error).color(theme::ERROR));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let can_start = self.plan().is_ok();
                        if ui
                            .add_enabled(
                                can_start,
                                theme::primary_button(if self.adds_to_queue {
                                    tr_str("Aggiungi alla coda", "Add to the queue")
                                } else {
                                    tr_str("Avvia conversione", "Start conversion")
                                }),
                            )
                            .clicked()
                        {
                            if let Ok(plan) = self.plan() {
                                outcome = DialogOutcome::Start(Box::new(plan));
                            }
                        }
                        if ui
                            .add(theme::secondary_button(tr_str("Annulla", "Cancel")))
                            .clicked()
                        {
                            outcome = DialogOutcome::Cancel;
                        }
                    });
                });
            });

        if modal.should_close() && matches!(outcome, DialogOutcome::None) {
            outcome = DialogOutcome::Cancel;
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
        tr_str(
            "8-bit · massima compatibilità",
            "8-bit · maximum compatibility",
        )
    } else {
        tr_str("10-bit · banding ridotto", "10-bit · less banding")
    }
}

fn order_name(value: &str) -> &'static str {
    match value {
        "largest" => tr_str(
            "Dal più grande (risparmio maggiore)",
            "Largest first (most to gain)",
        ),
        "smallest" => tr_str(
            "Dal più piccolo (risultati subito)",
            "Smallest first (quick results)",
        ),
        _ => tr_str("Come nell’elenco", "As in the list"),
    }
}

fn after_name(action: PowerAction) -> &'static str {
    match action {
        PowerAction::Nothing => tr_str("Non fare nulla", "Do nothing"),
        PowerAction::Sleep => tr_str("Metti il computer in stop", "Put the computer to sleep"),
        PowerAction::Shutdown => tr_str("Spegni il computer", "Shut the computer down"),
    }
}

fn content_name(value: &str) -> &'static str {
    match value {
        "anime" => tr_str("Anime / animazione", "Anime / animation"),
        "live" => tr_str("Film e serie TV", "Films and TV series"),
        _ => tr_str("Automatico (dal percorso)", "Automatic (from the path)"),
    }
}

fn kind_name(kind: MediaKind) -> &'static str {
    match kind {
        MediaKind::Anime => tr_str("anime", "anime"),
        MediaKind::Live => tr_str("film e serie TV", "films and TV series"),
    }
}

fn tune_label(tune: &str) -> &'static str {
    match tune {
        "animation" => "Animation",
        "grain" => "Grain",
        _ => tr_str("Nessuno", "None"),
    }
}

/// Name of a language setting: languages are shown in their own language.
fn language_name(value: &str) -> &'static str {
    match value {
        "it" => "Italiano",
        "en" => "English",
        _ => tr_str("Automatico (sistema)", "Automatic (system)"),
    }
}

fn container_label(container: &str) -> &'static str {
    if container == "mp4" {
        "MP4 · faststart"
    } else {
        tr_str("MKV · consigliato", "MKV · recommended")
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
    /// Space freed by the files that converted, known once the queue ends.
    pub space: Option<SpaceChange>,
    /// Set to hold the next file back; the one in progress finishes.
    pub pause: Arc<AtomicBool>,
    /// What to do with the computer when the queue has ended.
    pub after: PowerAction,
    pub after_handled: bool,
    /// The files of this batch, so they cannot be queued twice.
    pub sources: std::collections::HashSet<PathBuf>,
}

/// `, 1,2 GB instead of 2,0 GB (-40%)` for one converted file.
/// ` · subtitles do not fit MP4: saved as MKV` and the like.
fn message_note(result: &ConversionResult) -> String {
    if result.message.is_empty() {
        String::new()
    } else {
        format!(" · {}", result.message)
    }
}

fn size_note(result: &ConversionResult) -> String {
    let (Some(before), Some(after)) = (result.original_size, result.new_size) else {
        return String::new();
    };
    let change = SpaceChange { before, after };
    fotogramma_core::tr!(
        " · {} invece di {} ({})",
        " · {} instead of {} ({})",
        format_bytes(Some(after)),
        format_bytes(Some(before)),
        change.signed_percent()
    )
}

impl ConversionSession {
    pub fn new(
        plan: ConversionPlan,
        receiver: Receiver<ConversionMessage>,
        cancel: Arc<AtomicBool>,
    ) -> Self {
        let sources = plan.videos.iter().map(|video| video.path.clone()).collect();
        Self {
            sources,
            pause: plan.options.pause.clone(),
            after: plan.after,
            receiver,
            destination_dir: plan.destination_dir,
            replace_original: plan.replace_original,
            cancel,
            label: tr_str("Preparazione…", "Preparing…").to_string(),
            percent: 0,
            log: Vec::new(),
            results: Vec::new(),
            finished: false,
            failed: false,
            space: None,
            after_handled: false,
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
                    self.log.push(fotogramma_core::tr!(
                        "Avvio: {}\nOutput: {}",
                        "Start: {}\nOutput: {}",
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
                    eta,
                } => {
                    self.percent = percent;
                    let name = current_file
                        .file_name()
                        .map(|value| value.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.label = match eta {
                        Some(seconds) => fotogramma_core::tr!(
                            "{index} / {total} · {name} · {percent}% · circa {} rimasti",
                            "{index} / {total} · {name} · {percent}% · about {} left",
                            fotogramma_core::formatting::format_eta(seconds)
                        ),
                        None => format!("{index} / {total} · {name} · {percent}%"),
                    };
                }
                ConversionMessage::VerificationStarted {
                    index,
                    current_file,
                } => {
                    self.label = fotogramma_core::tr!(
                        "Verifica completa · {index} · {}",
                        "Full verification · {index} · {}",
                        current_file
                            .file_name()
                            .map(|value| value.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    );
                    self.log.push(
                        tr_str(
                            "Verifica completa prima della sostituzione…",
                            "Full verification before replacement…",
                        )
                        .to_string(),
                    );
                }
                ConversionMessage::FileCompleted(result) => {
                    let result = *result;
                    if result.cancelled {
                        self.log.push(fotogramma_core::tr!(
                            "Interrotto: {}",
                            "Stopped: {}",
                            result.source.display()
                        ));
                    } else if result.verified {
                        let note = format!("{}{}", size_note(&result), message_note(&result));
                        self.log.push(fotogramma_core::tr!(
                            "Verifica superata ✓ · originale sostituito: {}{note}",
                            "Verification passed ✓ · original replaced: {}{note}",
                            result.destination.display()
                        ));
                    } else if result.success {
                        let note = format!("{}{}", size_note(&result), message_note(&result));
                        self.log.push(fotogramma_core::tr!(
                            "Completato ✓ · {}{note}",
                            "Completed ✓ · {}{note}",
                            result.destination.display()
                        ));
                    } else if result.skipped {
                        self.log.push(fotogramma_core::tr!(
                            "Scartato · {}: {}",
                            "Dropped · {}: {}",
                            result.source.display(),
                            result.message
                        ));
                    } else {
                        self.log.push(fotogramma_core::tr!(
                            "Errore su {}: {}",
                            "Error on {}: {}",
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
                    let skipped = self.results.iter().filter(|result| result.skipped).count();
                    let errors = self.results.len().saturating_sub(successes + skipped);
                    let replacements = self.results.iter().filter(|result| result.replaced).count();
                    let mut summary = fotogramma_core::tr!(
                        "Conversione terminata: riuscite {successes}, errori {errors}",
                        "Conversion finished: {successes} succeeded, {errors} failed"
                    );
                    if skipped > 0 {
                        summary.push_str(&fotogramma_core::tr!(
                            " · scartati per risparmio troppo piccolo: {skipped}",
                            " · dropped for a saving that was too small: {skipped}"
                        ));
                    }
                    if replacements > 0 {
                        summary.push_str(&fotogramma_core::tr!(
                            " · originali sostituiti dopo verifica: {replacements}",
                            " · originals replaced after verification: {replacements}"
                        ));
                    }
                    self.label = summary;
                    self.percent = 100;
                    let mut space = SpaceChange::default();
                    for result in self.results.iter().filter(|result| result.success) {
                        if let (Some(before), Some(after)) = (result.original_size, result.new_size)
                        {
                            space.add(before, after);
                        }
                    }
                    self.space = (space.before > 0).then_some(space);
                }
                ConversionMessage::Failed(message) => {
                    self.log.push(message);
                    self.label =
                        tr_str("Conversione non riuscita", "Conversion failed").to_string();
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
    /// Hide the window and let the queue run in the background.
    Minimize,
}

pub fn show_progress(
    ctx: &egui::Context,
    session: &mut ConversionSession,
    waiting: usize,
) -> ProgressAction {
    let mut action = ProgressAction::None;
    let modal = egui::Modal::new(egui::Id::unique("conversion-progress"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(680.0);
            let heading = if session.failed {
                tr_str("Conversione non riuscita", "Conversion failed")
            } else if session.finished {
                tr_str("Conversione completata", "Conversion completed")
            } else {
                tr_str("Conversione in corso", "Conversion in progress")
            };
            ui.label(RichText::new(heading).size(theme::PAGE_TITLE_SIZE).strong());
            ui.label(&session.label);
            if waiting > 0 {
                ui.label(
                    RichText::new(fotogramma_core::tr!(
                        "In coda dopo questo: {} gruppi di video",
                        "Waiting after this one: {} batches",
                        waiting
                    ))
                    .small()
                    .color(theme::LAVENDER),
                );
            }
            if !session.finished && session.pause.load(std::sync::atomic::Ordering::Relaxed) {
                ui.label(
                    RichText::new(tr_str(
                        "In pausa: il file in corso finisce, poi la coda aspetta.",
                        "Paused: the file in progress finishes, then the queue waits.",
                    ))
                    .color(theme::CORAL_HOVER),
                );
            }
            if let Some(space) = session.space {
                let (text, color) = if space.is_saving() {
                    (
                        fotogramma_core::tr!(
                            "Spazio risparmiato: {} ({}) · da {} a {}",
                            "Space saved: {} ({}) · from {} to {}",
                            format_bytes(Some(space.delta())),
                            space.signed_percent(),
                            format_bytes(Some(space.before)),
                            format_bytes(Some(space.after))
                        ),
                        theme::MINT,
                    )
                } else {
                    (
                        fotogramma_core::tr!(
                            "Spazio in più: {} ({}) · da {} a {}",
                            "Extra space used: {} ({}) · from {} to {}",
                            format_bytes(Some(space.delta())),
                            space.signed_percent(),
                            format_bytes(Some(space.before)),
                            format_bytes(Some(space.after))
                        ),
                        theme::CORAL_HOVER,
                    )
                };
                ui.label(RichText::new(text).strong().color(color));
            }
            ui.add(
                egui::ProgressBar::new(f32::from(session.percent) / 100.0)
                    .desired_height(10.0)
                    .fill(theme::CORAL),
            );
            if session.replace_original && !session.finished {
                ui.label(
                    RichText::new(tr_str(
                        "Gli originali vengono toccati solo dopo la verifica completa.",
                        "The originals are touched only after full verification.",
                    ))
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
                if session.finished
                    && session.successes() > 0
                    && ui
                        .add(theme::secondary_button(tr_str(
                            "Apri cartella",
                            "Open folder",
                        )))
                        .clicked()
                {
                    action = ProgressAction::OpenFolder;
                }
                if !session.finished {
                    let paused = session.pause.load(std::sync::atomic::Ordering::Relaxed);
                    let label = if paused {
                        tr_str("Riprendi", "Resume")
                    } else {
                        tr_str("Pausa dopo il file", "Pause after this file")
                    };
                    if ui.add(theme::secondary_button(label)).clicked() {
                        session
                            .pause
                            .store(!paused, std::sync::atomic::Ordering::Relaxed);
                    }
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if session.finished {
                        if ui
                            .add(theme::primary_button(tr_str("Chiudi", "Close")))
                            .clicked()
                        {
                            action = ProgressAction::Close;
                        }
                    } else {
                        if ui
                            .add(theme::secondary_button(tr_str("Interrompi", "Stop")))
                            .clicked()
                        {
                            session.cancel();
                            action = ProgressAction::Cancel;
                        }
                        if ui
                            .add(theme::primary_button(tr_str(
                                "Riduci in background",
                                "Minimize to the background",
                            )))
                            .on_hover_text(tr_str(
                                "La conversione continua: nel frattempo puoi aggiungere altri video alla coda",
                                "The conversion keeps going: meanwhile you can add more videos to the queue",
                            ))
                            .clicked()
                        {
                            action = ProgressAction::Minimize;
                        }
                    }
                });
            });
        });

    if modal.should_close() && session.finished {
        action = ProgressAction::Close;
    }
    action
}

pub enum BarAction {
    None,
    Show,
    Pause,
    Stop,
}

/// The conversion in the background: one line at the bottom of the window.
pub fn show_background_bar(
    ui: &mut egui::Ui,
    session: &ConversionSession,
    waiting: usize,
) -> BarAction {
    let mut action = BarAction::None;
    ui.horizontal(|ui| {
        let (title, color) = if session.failed {
            (
                tr_str("Conversione non riuscita", "Conversion failed"),
                theme::ERROR,
            )
        } else if session.finished {
            (
                tr_str("Conversione completata", "Conversion completed"),
                theme::MINT,
            )
        } else if session.pause.load(std::sync::atomic::Ordering::Relaxed) {
            (
                tr_str("Conversione in pausa", "Conversion paused"),
                theme::CORAL_HOVER,
            )
        } else {
            (
                tr_str("Conversione in corso", "Conversion in progress"),
                theme::LAVENDER,
            )
        };
        ui.label(RichText::new(title).small().strong().color(color));
        ui.add(
            egui::ProgressBar::new(f32::from(session.percent) / 100.0)
                .desired_width(160.0)
                .desired_height(8.0)
                .fill(theme::CORAL),
        );
        ui.add(
            egui::Label::new(RichText::new(&session.label).small().color(theme::MUTED)).truncate(),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add(theme::secondary_button(if session.finished {
                    tr_str("Mostra risultati", "Show results")
                } else {
                    tr_str("Mostra", "Show")
                }))
                .clicked()
            {
                action = BarAction::Show;
            }
            if !session.finished {
                if ui
                    .add(theme::secondary_button(tr_str("Interrompi", "Stop")))
                    .clicked()
                {
                    action = BarAction::Stop;
                }
                let paused = session.pause.load(std::sync::atomic::Ordering::Relaxed);
                if ui
                    .add(theme::secondary_button(if paused {
                        tr_str("Riprendi", "Resume")
                    } else {
                        tr_str("Pausa", "Pause")
                    }))
                    .clicked()
                {
                    action = BarAction::Pause;
                }
            }
            if waiting > 0 {
                ui.label(
                    RichText::new(fotogramma_core::tr!("In coda: {}", "Queued: {}", waiting))
                        .small()
                        .color(theme::LAVENDER),
                );
            }
        });
    });
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
                    ui.label(tr_str("Percorso", "Path"));
                    ui.label(
                        RichText::new(video.path.display().to_string()).font(theme::mono(10.0)),
                    );
                    ui.end_row();
                    ui.label("Codec");
                    ui.label(codec_label(video.codec.as_deref()));
                    ui.end_row();
                    ui.label(tr_str("Durata", "Duration"));
                    ui.label(format_duration(video.duration));
                    ui.end_row();
                    ui.label(tr_str("Risoluzione", "Resolution"));
                    ui.label(format!(
                        "{} × {}",
                        video.width.unwrap_or(0),
                        video.height.unwrap_or(0)
                    ));
                    ui.end_row();
                    ui.label(tr_str("Dimensione", "Size"));
                    ui.label(format_bytes(video.size));
                    ui.end_row();
                    ui.label(tr_str("Stato", "Status"));
                    ui.label(video.status_label());
                    ui.end_row();
                });
            if let Some(error) = &video.error {
                ui.add_space(6.0);
                ui.label(RichText::new(error).color(theme::ERROR));
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(theme::primary_button(tr_str("Chiudi", "Close")))
                    .clicked()
                {
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

/// What is running when the user closes the window.
pub struct CloseContext {
    pub converting: bool,
    /// Batches waiting behind the running conversion.
    pub waiting: usize,
}

pub fn show_close_confirm(ctx: &egui::Context, running: &CloseContext) -> Option<CloseChoice> {
    let mut choice = None;
    let modal = egui::Modal::new(egui::Id::unique("close-confirm"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(480.0);
            let (title, question) = if running.converting {
                (
                    tr_str("Conversione in corso", "Conversion in progress"),
                    tr_str(
                        "Vuoi interrompere la conversione e chiudere Fotogramma?",
                        "Do you want to stop the conversion and close Fotogramma?",
                    ),
                )
            } else {
                (
                    tr_str("Analisi in corso", "Scan in progress"),
                    tr_str(
                        "Vuoi interrompere l’analisi e chiudere Fotogramma?",
                        "Do you want to stop the scan and close Fotogramma?",
                    ),
                )
            };
            ui.label(RichText::new(title).size(21.0).strong());
            ui.label(question);
            if running.converting && running.waiting > 0 {
                ui.label(
                    RichText::new(fotogramma_core::tr!(
                        "Anche i {} gruppi di video in coda andranno persi: alla prossima apertura non ripartiranno.",
                        "The {} queued batches of videos will be lost too: they will not start again next time.",
                        running.waiting
                    ))
                    .color(theme::CORAL_HOVER),
                );
            }
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(
                        egui::Button::new(
                            RichText::new(tr_str("Interrompi e chiudi", "Stop and close"))
                                .color(theme::CORAL_INK),
                        )
                        .fill(theme::CORAL),
                    )
                    .clicked()
                {
                    choice = Some(CloseChoice::StopAndClose);
                }
                if ui
                    .add(theme::secondary_button(tr_str("Continua", "Continue")))
                    .clicked()
                {
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

/// Preferences window. Edits `settings` in place; the return value tells the
/// caller whether something changed (to save and re-classify) and whether the
/// window was closed.
pub struct SettingsOutcome {
    pub changed: bool,
    pub check_now: bool,
    pub clear_cache: bool,
    pub closed: bool,
}

pub fn show_settings(ctx: &egui::Context, settings: &mut Settings) -> SettingsOutcome {
    let mut changed = false;
    let mut check_now = false;
    let mut clear_cache = false;
    let mut closed = false;
    let screen_height = ctx.content_rect().height();
    let modal_id = egui::Id::unique("settings-dialog");
    let modal_area = egui::Modal::default_area(modal_id).default_size(egui::vec2(
        560.0,
        (screen_height - 90.0).clamp(480.0, 800.0),
    ));
    let modal = egui::Modal::new(modal_id)
        .area(modal_area)
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.set_max_width(560.0);
            ui.label(
                RichText::new(tr_str("Impostazioni", "Settings"))
                    .size(21.0)
                    .strong(),
            );
            ui.label(
                RichText::new(tr_str(
                    "Valori predefiniti. Si applicano subito e vengono ricordati.",
                    "Default values. They apply immediately and are remembered.",
                ))
                .small()
                .color(theme::MUTED),
            );
            ui.add_space(8.0);

            let max_height = (screen_height - 150.0).max(240.0);
            egui::ScrollArea::vertical()
                .max_height(max_height)
                .auto_shrink([false, true])
                .show(ui, |ui| {
            ui.horizontal(|ui| {
                let language_label = ui.label(RichText::new(tr_str("Lingua", "Language")).strong());
                egui::ComboBox::from_id_salt("settings-language")
                    .width(240.0)
                    .selected_text(language_name(&settings.language))
                    .show_ui(ui, |ui| {
                        for value in ["auto", "it", "en"] {
                            if ui
                                .selectable_value(
                                    &mut settings.language,
                                    value.to_string(),
                                    language_name(value),
                                )
                                .changed()
                            {
                                changed = true;
                            }
                        }
                    })
                    .response
                    .labelled_by(language_label.id);
            });
            ui.add_space(8.0);

            ui.label(
                RichText::new(tr_str("Codec già accettati", "Already accepted codecs")).strong(),
            );
            ui.label(
                RichText::new(tr_str(
                    "I file con questi codec risultano compatibili e non vanno convertiti.",
                    "Files with these codecs count as compatible and are not converted.",
                ))
                .small()
                .color(theme::MUTED),
            );
            ui.horizontal_wrapped(|ui| {
                for (key, label) in KNOWN_CODECS {
                    let mut accepted = settings.accepted_codecs.iter().any(|value| value == key);
                    if ui.checkbox(&mut accepted, label).changed() {
                        settings.accepted_codecs.retain(|value| value != key);
                        if accepted {
                            settings.accepted_codecs.push(key.to_string());
                        }
                        changed = true;
                    }
                }
            });
            ui.add_space(8.0);

            ui.label(RichText::new(tr_str("Conversione", "Conversion")).strong());
            if ui
                .checkbox(
                    &mut settings.replace_original,
                    tr_str(
                        "Sostituisci l’originale per impostazione predefinita",
                        "Replace the original by default",
                    ),
                )
                .changed()
            {
                changed = true;
            }
            ui.label(
                RichText::new(tr_str(
                    "Resta valida la verifica completa con backup: vale solo per MKV e MP4.",
                    "Full verification with backup still applies: MKV and MP4 only.",
                ))
                .small()
                .color(theme::MUTED),
            );
            if ui
                .checkbox(
                    &mut settings.prevent_sleep,
                    tr_str(
                        "Evita lo stop del computer durante le conversioni",
                        "Keep the computer awake during conversions",
                    ),
                )
                .changed()
            {
                changed = true;
            }
            ui.horizontal(|ui| {
                let saving_label = ui.label(tr_str(
                    "Soglia di risparmio predefinita",
                    "Default minimum saving",
                ));
                if ui
                    .add(
                        egui::Slider::new(&mut settings.min_saving_percent, 0..=50)
                            .step_by(5.0)
                            .custom_formatter(|value, _| {
                                if value == 0.0 {
                                    tr_str("nessuna", "none").to_string()
                                } else {
                                    format!("{value}%")
                                }
                            }),
                    )
                    .labelled_by(saving_label.id)
                    .changed()
                {
                    changed = true;
                }
            });
            ui.label(
                RichText::new(tr_str(
                    "Se il nuovo file risparmia meno della soglia, si tiene l’originale e si scarta il risultato.",
                    "If the new file saves less than this, the original is kept and the result dropped.",
                ))
                .small()
                .color(theme::MUTED),
            );
            ui.add_space(4.0);

            egui::Grid::new("settings-form")
                .num_columns(2)
                .spacing([18.0, 6.0])
                .show(ui, |ui| {
                    let profile_label = ui.label(tr_str("Profilo", "Profile"));
                    let profiles = builtin_profiles();
                    let selected = profiles
                        .iter()
                        .find(|profile| profile.key == settings.default_profile)
                        .map(|profile| profile.label.clone())
                        .unwrap_or_else(|| tr_str("Bilanciato", "Balanced").to_string());
                    egui::ComboBox::from_id_salt("settings-profile")
                        .width(320.0)
                        .selected_text(selected)
                        .show_ui(ui, |ui| {
                            for profile in &profiles {
                                if ui
                                    .selectable_value(
                                        &mut settings.default_profile,
                                        profile.key.clone(),
                                        profile.label.clone(),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            }
                        })
                        .response
                        .labelled_by(profile_label.id);
                    ui.end_row();

                    let container_label_widget = ui.label(tr_str("Contenitore", "Container"));
                    egui::ComboBox::from_id_salt("settings-container")
                        .width(320.0)
                        .selected_text(container_label(&settings.default_container))
                        .show_ui(ui, |ui| {
                            for value in ["mkv", "mp4"] {
                                if ui
                                    .selectable_value(
                                        &mut settings.default_container,
                                        value.to_string(),
                                        container_label(value),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            }
                        })
                        .response
                        .labelled_by(container_label_widget.id);
                    ui.end_row();

                    let content_widget = ui.label(tr_str("Tipo di contenuto", "Content type"));
                    egui::ComboBox::from_id_salt("settings-content")
                        .width(320.0)
                        .selected_text(content_name(&settings.content_type))
                        .show_ui(ui, |ui| {
                            for value in ["auto", "anime", "live"] {
                                if ui
                                    .selectable_value(
                                        &mut settings.content_type,
                                        value.to_string(),
                                        content_name(value),
                                    )
                                    .changed()
                                {
                                    changed = true;
                                }
                            }
                        })
                        .response
                        .labelled_by(content_widget.id);
                    ui.end_row();
                });

            ui.add_space(4.0);
            let output_label = ui.label(tr_str(
                "Cartella di output predefinita",
                "Default output folder",
            ));
            ui.horizontal(|ui| {
                let edit = ui
                    .add(
                        egui::TextEdit::singleline(&mut settings.output_dir)
                            .hint_text(tr_str(
                                "Vuoto: cartella Fotogramma-H265 accanto agli originali",
                                "Empty: a Fotogramma-H265 folder next to the originals",
                            ))
                            .desired_width(300.0),
                    )
                    .labelled_by(output_label.id);
                if edit.changed() {
                    changed = true;
                }
                if ui
                    .add(theme::secondary_button(tr_str("Scegli…", "Choose…")))
                    .clicked()
                {
                    let start = if settings.output_dir.trim().is_empty() {
                        None
                    } else {
                        Some(PathBuf::from(settings.output_dir.trim()))
                    };
                    let mut dialog = rfd::FileDialog::new();
                    if let Some(start) = start {
                        dialog = dialog.set_directory(start);
                    }
                    if let Some(folder) = dialog.pick_folder() {
                        settings.output_dir = folder.to_string_lossy().into_owned();
                        changed = true;
                    }
                }
            });

            ui.add_space(8.0);
            ui.label(RichText::new(tr_str("Scansione", "Scanning")).strong());
            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut settings.scan_cache,
                        tr_str(
                            "Ricorda i risultati per scansioni più veloci",
                            "Remember results for faster scans",
                        ),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .add(theme::secondary_button(tr_str(
                        "Svuota la cache",
                        "Clear the cache",
                    )))
                    .clicked()
                {
                    clear_cache = true;
                }
            });
            ui.label(
                RichText::new(tr_str(
                    "Una nuova scansione rilegge solo i file cambiati (stesso percorso, dimensione e data).",
                    "A new scan only reads the files that changed (same path, size and date).",
                ))
                .small()
                .color(theme::MUTED),
            );

            ui.add_space(8.0);
            ui.label(RichText::new(tr_str("Aggiornamenti", "Updates")).strong());
            ui.horizontal(|ui| {
                if ui
                    .checkbox(
                        &mut settings.check_updates,
                        tr_str(
                            "Cerca aggiornamenti all’avvio",
                            "Check for updates at startup",
                        ),
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .add(theme::secondary_button(tr_str("Controlla ora", "Check now")))
                    .clicked()
                {
                    check_now = true;
                }
            });
            ui.label(
                RichText::new(tr_str(
                    "Contatta solo GitHub per leggere l’ultima versione. Nessun dato sulla libreria viene inviato.",
                    "Only contacts GitHub to read the latest version. Nothing about your library is sent.",
                ))
                .small()
                .color(theme::MUTED),
            );

                });

            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(theme::primary_button(tr_str("Chiudi", "Close")))
                    .clicked()
                {
                    closed = true;
                }
                if ui
                    .add(theme::secondary_button(tr_str("Ripristina", "Reset")))
                    .clicked()
                {
                    let directory = std::mem::take(&mut settings.directory);
                    let (width, height) = (settings.window_width, settings.window_height);
                    *settings = Settings::default();
                    settings.directory = directory;
                    settings.window_width = width;
                    settings.window_height = height;
                    changed = true;
                }
            });
        });
    if modal.should_close() {
        closed = true;
    }
    SettingsOutcome {
        changed,
        check_now,
        clear_cache,
        closed,
    }
}

/// What the update window is showing.
pub enum UpdateStage {
    Available,
    Downloading {
        done: u64,
        total: u64,
    },
    /// Unpacked and verified; waiting for the restart.
    Ready(PathBuf),
    Failed(String),
}

pub struct UpdateDialog {
    pub release: Release,
    pub stage: UpdateStage,
    pub target: Option<InstallTarget>,
    pub receiver: Option<Receiver<InstallMessage>>,
    pub cancel: Arc<AtomicBool>,
}

pub enum UpdateAction {
    None,
    Install,
    Cancel,
    Restart,
    Later,
    Skip,
}

impl UpdateDialog {
    pub fn new(release: Release, target: Option<InstallTarget>) -> Self {
        Self {
            release,
            stage: UpdateStage::Available,
            target,
            receiver: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Whether this copy can replace itself with the release.
    pub fn can_install(&self) -> bool {
        self.target.is_some() && self.release.package.is_some() && self.release.sums_url.is_some()
    }

    pub fn poll(&mut self) {
        let Some(receiver) = &self.receiver else {
            return;
        };
        while let Ok(message) = receiver.try_recv() {
            self.stage = match message {
                InstallMessage::Progress { done, total } => {
                    UpdateStage::Downloading { done, total }
                }
                InstallMessage::Staged(path) => UpdateStage::Ready(path),
                InstallMessage::Failed(error) => UpdateStage::Failed(error),
            };
        }
    }
}

/// The "new version available" window. `busy` is true while a scan or a
/// conversion runs: the restart is held back until they end.
pub fn show_update(ctx: &egui::Context, dialog: &mut UpdateDialog, busy: bool) -> UpdateAction {
    let mut action = UpdateAction::None;
    let modal = egui::Modal::new(egui::Id::unique("update-dialog"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(560.0);
            ui.set_max_width(640.0);
            ui.label(
                RichText::new(tr_str("Aggiornamento disponibile", "Update available"))
                    .size(21.0)
                    .strong(),
            );
            ui.label(format!(
                "{} → {}",
                env!("CARGO_PKG_VERSION"),
                dialog.release.version
            ));
            ui.add_space(4.0);
            let notes = if dialog.release.notes.is_empty() {
                tr_str("Nessuna nota di rilascio.", "No release notes.").to_string()
            } else {
                dialog.release.notes.chars().take(6000).collect()
            };
            egui::ScrollArea::vertical()
                .max_height(280.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    crate::markdown::show(ui, &notes);
                });
            ui.add_space(6.0);

            if !dialog.can_install() {
                let hint = ui.label(
                    RichText::new(tr_str(
                        "Questa copia non può aggiornarsi da sola: scarica la nuova versione da",
                        "This copy cannot update itself: download the new version from",
                    ))
                    .small(),
                );
                let mut page = dialog.release.page_url.clone();
                ui.add(
                    egui::TextEdit::singleline(&mut page)
                        .font(theme::mono(10.0))
                        .desired_width(f32::INFINITY),
                )
                .labelled_by(hint.id);
            }

            match &dialog.stage {
                UpdateStage::Available => {}
                UpdateStage::Downloading { done, total } => {
                    let fraction = if *total == 0 {
                        0.0
                    } else {
                        (*done as f32 / *total as f32).min(1.0)
                    };
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_height(10.0)
                            .fill(theme::CORAL)
                            .text(format!("{}%", (fraction * 100.0).round() as u32)),
                    );
                    ui.label(
                        RichText::new(tr_str(
                            "Download e verifica in corso…",
                            "Downloading and verifying…",
                        ))
                        .small()
                        .color(theme::MUTED),
                    );
                }
                UpdateStage::Ready(_) => {
                    ui.label(
                        RichText::new(tr_str(
                            "Scaricato e verificato. Riavvia per completare l’aggiornamento.",
                            "Downloaded and verified. Restart to finish the update.",
                        ))
                        .color(theme::MINT),
                    );
                    if busy {
                        ui.label(
                            RichText::new(tr_str(
                                "Termina o interrompi l’analisi o la conversione in corso.",
                                "Finish or stop the running scan or conversion first.",
                            ))
                            .small()
                            .color(theme::CORAL_HOVER),
                        );
                    }
                }
                UpdateStage::Failed(error) => {
                    ui.label(RichText::new(error).color(theme::ERROR));
                }
            }

            ui.add_space(8.0);
            ui.with_layout(
                egui::Layout::right_to_left(egui::Align::Center),
                |ui| match &dialog.stage {
                    UpdateStage::Available | UpdateStage::Failed(_) => {
                        if ui
                            .add_enabled(
                                dialog.can_install(),
                                theme::primary_button(tr_str(
                                    "Aggiorna e riavvia",
                                    "Update and restart",
                                )),
                            )
                            .clicked()
                        {
                            action = UpdateAction::Install;
                        }
                        if ui
                            .add(theme::secondary_button(tr_str("Più tardi", "Later")))
                            .clicked()
                        {
                            action = UpdateAction::Later;
                        }
                        if ui
                            .add(theme::secondary_button(tr_str(
                                "Salta questa versione",
                                "Skip this version",
                            )))
                            .clicked()
                        {
                            action = UpdateAction::Skip;
                        }
                    }
                    UpdateStage::Downloading { .. } => {
                        if ui
                            .add(theme::secondary_button(tr_str("Annulla", "Cancel")))
                            .clicked()
                        {
                            action = UpdateAction::Cancel;
                        }
                    }
                    UpdateStage::Ready(_) => {
                        if ui
                            .add_enabled(
                                !busy,
                                theme::primary_button(tr_str("Riavvia ora", "Restart now")),
                            )
                            .clicked()
                        {
                            action = UpdateAction::Restart;
                        }
                        if ui
                            .add(theme::secondary_button(tr_str("Più tardi", "Later")))
                            .clicked()
                        {
                            action = UpdateAction::Later;
                        }
                    }
                },
            );
        });
    if modal.should_close() && matches!(action, UpdateAction::None) {
        action = UpdateAction::Later;
    }
    action
}

/// How the built-in profiles differ, so the choice is not a guess. The times
/// are relative to Intermediate and indicative: they depend on the video and
/// on the processor.
fn profile_comparison(ui: &mut egui::Ui) {
    egui::CollapsingHeader::new(
        RichText::new(tr_str("Quale profilo scegliere?", "Which profile to choose?"))
            .small()
            .color(theme::LAVENDER),
    )
    .id_salt("profile-comparison")
    .default_open(false)
    .show(ui, |ui| {
        let rows: [(&str, &str, &str, &str); 4] = [
            (
                "max_quality",
                tr_str("circa 6×", "about 6×"),
                tr_str("il più piccolo", "the smallest"),
                tr_str(
                    "Solo se il tempo non conta: la differenza con Alta qualità è minima.",
                    "Only if time does not matter: the difference from High quality is tiny.",
                ),
            ),
            (
                "high_quality",
                tr_str("circa 2×", "about 2×"),
                tr_str("quasi come il massimo", "almost as small as Maximum"),
                tr_str(
                    "La scelta per un archivio: quasi la stessa qualità in un terzo del tempo.",
                    "The choice for an archive: nearly the same quality in a third of the time.",
                ),
            ),
            (
                "balanced",
                tr_str("1×", "1×"),
                tr_str("più grande del massimo", "larger than Maximum"),
                tr_str(
                    "Per molti file o poco tempo: buona qualità, file un po' meno compresso.",
                    "For many files or little time: good quality, a slightly less compressed file.",
                ),
            ),
            (
                "fast",
                tr_str("circa 0,1×", "about 0.1×"),
                tr_str("il più grande", "the largest"),
                tr_str(
                    "Per una prova o per liberare il PC: qualità più bassa e poco spazio risparmiato.",
                    "For a test or to free the PC: lower quality and little space saved.",
                ),
            ),
        ];
        egui::Grid::new("profile-comparison-grid")
            .num_columns(4)
            .spacing([14.0, 4.0])
            .show(ui, |ui| {
                for title in [
                    tr_str("Profilo", "Profile"),
                    tr_str("Tempo", "Time"),
                    tr_str("File", "File"),
                    tr_str("Quando usarlo", "When to use it"),
                ] {
                    ui.label(RichText::new(title).small().color(theme::LAVENDER));
                }
                ui.end_row();
                for (key, time, size, when) in rows {
                    let name = builtin_profile(key)
                        .map(|profile| profile.label)
                        .unwrap_or_default();
                    ui.label(RichText::new(name).small());
                    ui.label(RichText::new(time).small().color(theme::MUTED));
                    ui.label(RichText::new(size).small().color(theme::MUTED));
                    ui.add(
                        egui::Label::new(RichText::new(when).small().color(theme::MUTED)).wrap(),
                    );
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
        ui.label(
            RichText::new(tr_str(
                "Tempi indicativi rispetto a Intermedio: cambiano con il video e con il processore. Una conversione parte da una stima prudente che migliora dopo qualche minuto. Il CRF più basso dà più qualità e file più grande; il preset più lento comprime meglio a parità di qualità.",
                "Times are indicative, relative to Intermediate: they change with the video and the processor. A conversion starts from a cautious estimate that improves after a few minutes. A lower CRF gives more quality and a larger file; a slower preset compresses better at the same quality.",
            ))
            .small()
            .color(theme::MUTED),
        );
    });
}

/// "About Fotogramma": version, license and what is bundled. True when closed.
pub fn show_about(ctx: &egui::Context, logo: Option<&egui::TextureHandle>) -> bool {
    let mut closed = false;
    let modal = egui::Modal::new(egui::Id::unique("about-dialog"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(420.0);
            ui.set_max_width(480.0);
            ui.horizontal(|ui| {
                if let Some(logo) = logo {
                    ui.add(egui::Image::new(logo).fit_to_exact_size(egui::Vec2::splat(64.0)));
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("Fotogramma").size(26.0).strong());
                    ui.label(
                        RichText::new(fotogramma_core::tr!(
                            "Versione {}",
                            "Version {}",
                            env!("CARGO_PKG_VERSION")
                        ))
                        .color(theme::MUTED),
                    );
                    ui.label(RichText::new("H.265 cinema library").small().color(theme::MUTED));
                });
            });
            ui.add_space(8.0);
            ui.label(tr_str(
                "Trova i video non ancora H.265 e li converte con FFmpeg, con il controllo di ogni passaggio.",
                "Finds the videos that are not H.265 yet and converts them with FFmpeg, with control over every step.",
            ));
            ui.add_space(6.0);
            for text in [
                tr_str(
                    "Programma distribuito con licenza MIT, «così com’è», senza garanzie.",
                    "Released under the MIT License, “as is”, without warranty.",
                ),
                tr_str(
                    "Include FFmpeg e x265, compilati con --enable-gpl (GPL v2 o successiva).",
                    "Includes FFmpeg and x265, built with --enable-gpl (GPL v2 or later).",
                ),
                tr_str(
                    "Font: Outfit e IBM Plex Mono (SIL Open Font License).",
                    "Fonts: Outfit and IBM Plex Mono (SIL Open Font License).",
                ),
                tr_str(
                    "Non invia dati sulla tua libreria. L’unica richiesta di rete è il controllo degli aggiornamenti.",
                    "Sends nothing about your library. The only network request is the update check.",
                ),
            ] {
                ui.label(RichText::new(text).small().color(theme::MUTED));
            }
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(theme::primary_button(tr_str("Chiudi", "Close")))
                    .clicked()
                {
                    closed = true;
                }
            });
        });
    closed || modal.should_close()
}

/// The answer to "Check for updates…" when there is no update to offer.
#[derive(Clone, Debug, PartialEq)]
pub enum UpdateNotice {
    Checking,
    UpToDate,
    Failed(String),
}

/// A small friendly card with the result of a manual update check. True when
/// the user closed it.
pub fn show_update_notice(ctx: &egui::Context, notice: &UpdateNotice) -> bool {
    let mut closed = false;
    let modal = egui::Modal::new(egui::Id::unique("update-notice"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_width(340.0);
            ui.vertical_centered(|ui| {
                ui.add_space(6.0);
                match notice {
                    UpdateNotice::Checking => {
                        ui.add(egui::Spinner::new().size(40.0).color(theme::CORAL));
                    }
                    UpdateNotice::UpToDate => badge(ui, theme::MINT, true),
                    UpdateNotice::Failed(_) => badge(ui, theme::CORAL, false),
                }
                ui.add_space(12.0);
                let (title, detail) = match notice {
                    UpdateNotice::Checking => (
                        tr_str("Controllo in corso…", "Checking…").to_string(),
                        tr_str(
                            "Cerco una versione più recente su GitHub.",
                            "Looking for a newer version on GitHub.",
                        )
                        .to_string(),
                    ),
                    UpdateNotice::UpToDate => (
                        tr_str("Sei aggiornato", "You are up to date").to_string(),
                        fotogramma_core::tr!(
                            "Fotogramma {} è l’ultima versione disponibile.",
                            "Fotogramma {} is the latest version.",
                            env!("CARGO_PKG_VERSION")
                        ),
                    ),
                    UpdateNotice::Failed(error) => (
                        tr_str("Controllo non riuscito", "Check failed").to_string(),
                        error.clone(),
                    ),
                };
                ui.label(RichText::new(title).size(22.0).strong());
                ui.add_space(2.0);
                ui.add(egui::Label::new(RichText::new(detail).small().color(theme::MUTED)).wrap());
                ui.add_space(14.0);
                let label = match notice {
                    UpdateNotice::Checking => tr_str("Annulla", "Cancel"),
                    UpdateNotice::UpToDate => tr_str("Perfetto", "Great"),
                    UpdateNotice::Failed(_) => tr_str("Chiudi", "Close"),
                };
                let button = if matches!(notice, UpdateNotice::Checking) {
                    theme::secondary_button(label)
                } else {
                    theme::primary_button(label)
                };
                if ui.add(button).clicked() {
                    closed = true;
                }
            });
        });
    closed || modal.should_close()
}

/// A round badge: a check mark, or an exclamation mark.
fn badge(ui: &mut egui::Ui, color: egui::Color32, check: bool) {
    let (rect, _) = ui.allocate_exact_size(egui::Vec2::splat(64.0), egui::Sense::hover());
    let painter = ui.painter();
    let centre = rect.center();
    painter.circle_filled(centre, 32.0, color.gamma_multiply(0.16));
    painter.circle_stroke(centre, 30.0, egui::Stroke::new(2.0, color));
    let stroke = egui::Stroke::new(4.0, color);
    let at = |x: f32, y: f32| egui::pos2(rect.left() + x * 64.0, rect.top() + y * 64.0);
    if check {
        painter.add(egui::Shape::line(
            vec![at(0.30, 0.52), at(0.45, 0.66), at(0.71, 0.36)],
            stroke,
        ));
        painter.circle_filled(at(0.30, 0.52), 2.0, color);
        painter.circle_filled(at(0.71, 0.36), 2.0, color);
    } else {
        painter.line_segment([at(0.5, 0.28), at(0.5, 0.56)], stroke);
        painter.circle_filled(at(0.5, 0.70), 2.8, color);
    }
}

/// The first-run question. `Some(true)` to check at startup, `Some(false)` not to.
pub fn show_update_prompt(ctx: &egui::Context) -> Option<bool> {
    let mut choice = None;
    egui::Modal::new(egui::Id::unique("update-prompt"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.set_max_width(520.0);
            ui.label(
                RichText::new(tr_str("Aggiornamenti automatici?", "Automatic updates?"))
                    .size(21.0)
                    .strong(),
            );
            ui.add_space(4.0);
            ui.label(tr_str(
                "Fotogramma può controllare all’avvio se è uscita una versione nuova e proporti di installarla.",
                "Fotogramma can check at startup whether a new version is out and offer to install it.",
            ));
            ui.add_space(6.0);
            ui.label(
                RichText::new(tr_str(
                    "È l’unica richiesta di rete dell’app: va a GitHub e non invia nulla sulla tua libreria. Finché non rispondi sì, non parte nessuna richiesta. Puoi cambiare idea in Impostazioni.",
                    "It is the app’s only network request: it goes to GitHub and sends nothing about your library. Until you say yes, no request is made. You can change your mind in Settings.",
                ))
                .small()
                .color(theme::MUTED),
            );
            ui.add_space(10.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(theme::primary_button(tr_str(
                        "Sì, controlla all’avvio",
                        "Yes, check at startup",
                    )))
                    .clicked()
                {
                    choice = Some(true);
                }
                if ui
                    .add(theme::secondary_button(tr_str("No, grazie", "No, thanks")))
                    .clicked()
                {
                    choice = Some(false);
                }
            });
        });
    choice
}

/// "Interrupted conversions: resume?" shown at startup.
pub enum ResumeChoice {
    Resume,
    Discard,
}

pub fn show_resume(ctx: &egui::Context, remaining: usize) -> Option<ResumeChoice> {
    let mut choice = None;
    egui::Modal::new(egui::Id::unique("resume-queue"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(480.0);
            ui.label(
                RichText::new(tr_str("Conversioni interrotte", "Interrupted conversions"))
                    .size(21.0)
                    .strong(),
            );
            ui.label(fotogramma_core::tr!(
                "L’ultima coda non è finita: restano {} video. Vuoi riprenderla?",
                "The last queue did not finish: {} videos are left. Resume it?",
                fotogramma_core::formatting::format_int(remaining)
            ));
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(theme::primary_button(tr_str("Riprendi", "Resume")))
                    .clicked()
                {
                    choice = Some(ResumeChoice::Resume);
                }
                if ui
                    .add(theme::secondary_button(tr_str("Scarta", "Discard")))
                    .clicked()
                {
                    choice = Some(ResumeChoice::Discard);
                }
            });
        });
    choice
}

pub enum PowerChoice {
    Now,
    Cancel,
}

/// The countdown before the computer sleeps or shuts down.
pub fn show_power_countdown(
    ctx: &egui::Context,
    action: PowerAction,
    seconds_left: u64,
) -> Option<PowerChoice> {
    let mut choice = None;
    egui::Modal::new(egui::Id::unique("power-countdown"))
        .frame(modal_frame())
        .show(ctx, |ui| {
            ui.set_min_width(480.0);
            ui.label(
                RichText::new(tr_str("Conversione terminata", "Conversion finished"))
                    .size(21.0)
                    .strong(),
            );
            ui.label(fotogramma_core::tr!(
                "{} tra {} secondi.",
                "{} in {} seconds.",
                after_name(action),
                seconds_left
            ));
            ui.add_space(8.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(theme::primary_button(tr_str("Annulla", "Cancel")))
                    .clicked()
                {
                    choice = Some(PowerChoice::Cancel);
                }
                if ui
                    .add(theme::secondary_button(tr_str("Ora", "Now")))
                    .clicked()
                {
                    choice = Some(PowerChoice::Now);
                }
            });
        });
    choice
}

#[cfg(test)]
mod tests {
    use super::*;
    use fotogramma_core::conversion::build_ffmpeg_args;
    use fotogramma_core::scanner::Category;

    fn video(path: &str) -> VideoInfo {
        VideoInfo {
            hdr: false,
            path: PathBuf::from(path),
            relative_path: "a.mkv".into(),
            name: "a.mkv".into(),
            folder: String::new(),
            extension: "mkv".into(),
            category: Category::Convert,
            codec: Some("h264".into()),
            size: Some(1),
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            error: None,
        }
    }

    fn arguments(dialog: &ConversionDialog) -> Vec<String> {
        build_ffmpeg_args(
            &dialog.current_profile(),
            Path::new("/in/a.mkv"),
            Path::new("/out/a.h265.mkv"),
        )
    }

    fn has(args: &[String], pair: [&str; 2]) -> bool {
        args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1])
    }

    #[test]
    fn an_anime_folder_selects_the_animation_tune() {
        let dialog =
            ConversionDialog::new(vec![video("/Media/Anime/Show/a.mkv")], &Settings::default());
        assert_eq!(dialog.effective_kind(), Some(MediaKind::Anime));
        assert!(has(&arguments(&dialog), ["-tune", "animation"]));
    }

    #[test]
    fn live_action_and_unknown_paths_use_no_tune() {
        for path in ["/Media/Film/a.mkv", "/Media/Misc/a.mkv"] {
            let dialog = ConversionDialog::new(vec![video(path)], &Settings::default());
            assert!(
                !arguments(&dialog).iter().any(|value| value == "-tune"),
                "{path}"
            );
        }
    }

    #[test]
    fn the_user_choice_beats_the_path() {
        let mut settings = Settings {
            content_type: "live".to_string(),
            ..Settings::default()
        };
        let dialog = ConversionDialog::new(vec![video("/Media/Anime/a.mkv")], &settings);
        assert_eq!(dialog.effective_kind(), Some(MediaKind::Live));
        assert!(!arguments(&dialog).iter().any(|value| value == "-tune"));
        settings.content_type = "anime".to_string();
        let dialog = ConversionDialog::new(vec![video("/Media/Film/a.mkv")], &settings);
        assert!(has(&arguments(&dialog), ["-tune", "animation"]));
    }

    #[test]
    fn x265_never_receives_the_film_tune() {
        for content in ["auto", "anime", "live"] {
            let settings = Settings {
                content_type: content.to_string(),
                ..Settings::default()
            };
            for path in ["/Media/Anime/a.mkv", "/Media/Film/a.mkv", "/x/a.mkv"] {
                let dialog = ConversionDialog::new(vec![video(path)], &settings);
                assert!(!has(&arguments(&dialog), ["-tune", "film"]));
            }
        }
    }

    #[test]
    fn the_default_output_folder_is_used_when_set() {
        let mut settings = Settings::default();
        let next_to_source = ConversionDialog::new(vec![video("/Media/a.mkv")], &settings);
        // Compare paths, not strings: Windows joins with a backslash.
        assert_eq!(
            PathBuf::from(&next_to_source.destination),
            PathBuf::from("/Media").join("Fotogramma-H265")
        );
        settings.output_dir = "  /Volumes/Out  ".to_string();
        let custom = ConversionDialog::new(vec![video("/Media/a.mkv")], &settings);
        assert_eq!(
            PathBuf::from(&custom.destination),
            PathBuf::from("/Volumes/Out")
        );
    }

    #[test]
    fn the_audio_choice_applies_to_the_built_in_profiles() {
        let mut dialog = ConversionDialog::new(vec![video("/Media/a.mkv")], &Settings::default());
        dialog.audio_index = 2;
        let args = arguments(&dialog);
        assert!(has(&args, ["-c:a", "aac"]));
        assert!(has(&args, ["-b:a", "160k"]));
    }

    fn finished_result(
        name: &str,
        before: Option<u64>,
        after: Option<u64>,
        ok: bool,
    ) -> ConversionResult {
        ConversionResult {
            source: PathBuf::from(format!("/in/{name}")),
            destination: PathBuf::from(format!("/out/{name}")),
            success: ok,
            message: String::new(),
            cancelled: false,
            verified: false,
            replaced: false,
            original_size: before,
            new_size: after,
            skipped: false,
        }
    }

    fn session_after(results: Vec<ConversionResult>) -> ConversionSession {
        let (sender, receiver) = std::sync::mpsc::channel();
        let plan = ConversionPlan {
            videos: Vec::new(),
            profile: builtin_profile("balanced").unwrap(),
            destination_dir: PathBuf::from("/out"),
            container: "mkv".to_string(),
            replace_original: false,
            options: ConversionOptions::default(),
            after: PowerAction::Nothing,
        };
        let mut session = ConversionSession::new(plan, receiver, Arc::new(AtomicBool::new(false)));
        for result in &results {
            sender
                .send(ConversionMessage::FileCompleted(Box::new(result.clone())))
                .unwrap();
        }
        sender.send(ConversionMessage::Completed(results)).unwrap();
        session.poll();
        session
    }

    #[test]
    fn the_session_adds_up_the_space_saved_by_the_files_that_converted() {
        let session = session_after(vec![
            finished_result("a.mkv", Some(2_000), Some(800), true),
            finished_result("b.mkv", Some(1_000), Some(400), true),
            // A failure frees nothing and must not count.
            finished_result("c.mkv", Some(9_000), None, false),
        ]);
        let space = session.space.expect("a summary");
        assert_eq!((space.before, space.after), (3_000, 1_200));
        assert!(space.is_saving());
        assert_eq!(space.signed_percent(), "-60%");
    }

    #[test]
    fn each_finished_file_says_how_much_smaller_it_is() {
        let session = session_after(vec![finished_result("a.mkv", Some(2_000), Some(800), true)]);
        let line = session
            .log
            .iter()
            .find(|line| line.contains("a.mkv"))
            .unwrap();
        assert!(line.contains("-60%"), "{line}");
        assert!(line.contains("invece di"), "{line}");
    }

    #[test]
    fn a_result_that_grew_is_reported_as_growth() {
        let session = session_after(vec![finished_result(
            "a.mkv",
            Some(1_000),
            Some(1_300),
            true,
        )]);
        let space = session.space.unwrap();
        assert!(!space.is_saving());
        assert_eq!(space.signed_percent(), "+30%");
    }

    #[test]
    fn no_sizes_means_no_summary() {
        let session = session_after(vec![finished_result("a.mkv", None, None, true)]);
        assert!(session.space.is_none());
    }
    #[test]
    fn the_order_option_sorts_the_queue() {
        let mut small = video("/Media/a.mkv");
        small.size = Some(10);
        let mut large = video("/Media/b.mkv");
        large.size = Some(900);
        let mut medium = video("/Media/c.mkv");
        medium.size = Some(300);
        let names = |dialog: &ConversionDialog| {
            dialog
                .plan()
                .unwrap()
                .videos
                .iter()
                .map(|v| v.path.file_name().unwrap().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        };
        let mut dialog = ConversionDialog::new(
            vec![small.clone(), large.clone(), medium.clone()],
            &Settings::default(),
        );
        assert_eq!(dialog.order, "list");
        assert_eq!(
            names(&dialog),
            vec!["a.mkv", "b.mkv", "c.mkv"],
            "as selected"
        );
        dialog.order = "largest".into();
        assert_eq!(dialog.plan().unwrap().videos[0].size, Some(900));
        assert_eq!(dialog.plan().unwrap().videos[2].size, Some(10));
        dialog.order = "smallest".into();
        assert_eq!(dialog.plan().unwrap().videos[0].size, Some(10));
    }

    #[test]
    fn a_saved_preset_comes_back_with_every_setting() {
        let mut dialog = ConversionDialog::new(vec![video("/Media/a.mkv")], &Settings::default());
        dialog.profile_key = "custom".into();
        dialog.crf = 19;
        dialog.preset = "slower".into();
        dialog.pixel = "yuv420p10le".into();
        dialog.tune = "grain".into();
        dialog.aq = Some(2);
        dialog.audio_index = 2;
        dialog.container = "mp4".into();
        assert!(dialog.save_preset("  Film grain  "));
        assert!(dialog.presets_dirty);
        assert_eq!(dialog.presets.len(), 1);
        assert_eq!(dialog.presets[0].name, "Film grain");

        // Something else is chosen, then the preset is loaded back.
        dialog.profile_key = "balanced".into();
        dialog.crf = 30;
        dialog.tune = String::new();
        assert!(dialog.load_preset("Film grain"));
        assert_eq!(dialog.profile_key, "custom");
        assert_eq!(
            (dialog.crf, dialog.preset.as_str(), dialog.tune.as_str()),
            (19, "slower", "grain")
        );
        assert_eq!(
            (dialog.aq, dialog.audio_index, dialog.container.as_str()),
            (Some(2), 2, "mp4")
        );
        let args = arguments(&dialog);
        assert!(has(&args, ["-crf", "19"]) && has(&args, ["-tune", "grain"]));
        assert!(!dialog.load_preset("missing"));
    }

    #[test]
    fn saving_under_the_same_name_replaces_and_blank_names_are_refused() {
        let mut dialog = ConversionDialog::new(vec![video("/Media/a.mkv")], &Settings::default());
        dialog.crf = 18;
        assert!(dialog.save_preset("Mine"));
        dialog.crf = 25;
        assert!(dialog.save_preset("Mine"));
        assert_eq!(dialog.presets.len(), 1);
        assert_eq!(dialog.presets[0].crf, 25);
        assert!(!dialog.save_preset("   "));
        assert_eq!(dialog.presets.len(), 1);
        assert!(dialog.save_preset(&"x".repeat(100)));
        assert_eq!(
            dialog.presets[1].name.chars().count(),
            40,
            "long names are cut"
        );
    }

    #[test]
    fn deleting_a_preset_forgets_it() {
        let mut dialog = ConversionDialog::new(vec![video("/Media/a.mkv")], &Settings::default());
        dialog.save_preset("A");
        dialog.save_preset("B");
        dialog.presets_dirty = false;
        dialog.delete_preset("A");
        assert_eq!(
            dialog
                .presets
                .iter()
                .map(|p| p.name.as_str())
                .collect::<Vec<_>>(),
            vec!["B"]
        );
        assert!(dialog.presets_dirty);
        dialog.presets_dirty = false;
        dialog.delete_preset("nope");
        assert!(!dialog.presets_dirty);
    }

    #[test]
    fn saved_presets_from_the_settings_are_offered() {
        let settings = Settings {
            presets: vec![SavedPreset {
                name: "Anime 1080p".into(),
                crf: 20,
                preset: "slow".into(),
                pixel: "yuv420p10le".into(),
                tune: "animation".into(),
                aq: Some(3),
                audio_index: 0,
                container: "mkv".into(),
            }],
            ..Settings::default()
        };
        let mut dialog = ConversionDialog::new(vec![video("/Media/a.mkv")], &settings);
        assert_eq!(dialog.presets.len(), 1);
        assert!(dialog.load_preset("Anime 1080p"));
        assert!(has(&arguments(&dialog), ["-tune", "animation"]));
    }
}
