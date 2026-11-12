//! The table of a conversion queue's results: every file with its size before
//! and after and the space it saved.

use eframe::egui::{self, Align, Label, RichText};
use egui_extras::{Column, TableBuilder};
use fotogramma_core::formatting::{format_bytes, SpaceChange};
use fotogramma_core::i18n::tr_str;

use crate::theme;
use crate::worker::ConversionResult;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Converted,
    Replaced,
    Dropped,
    Stopped,
    Failed,
}

impl Outcome {
    pub fn of(result: &ConversionResult) -> Self {
        if result.cancelled {
            Outcome::Stopped
        } else if result.replaced {
            Outcome::Replaced
        } else if result.success {
            Outcome::Converted
        } else if result.skipped {
            Outcome::Dropped
        } else {
            Outcome::Failed
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Outcome::Converted => tr_str("Convertito", "Converted"),
            Outcome::Replaced => tr_str("Sostituito", "Replaced"),
            Outcome::Dropped => tr_str("Scartato", "Dropped"),
            Outcome::Stopped => tr_str("Interrotto", "Stopped"),
            Outcome::Failed => tr_str("Errore", "Error"),
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            Outcome::Converted | Outcome::Replaced => theme::MINT,
            Outcome::Dropped | Outcome::Stopped => theme::LAVENDER,
            Outcome::Failed => theme::ERROR,
        }
    }

    fn order(self) -> u8 {
        match self {
            Outcome::Converted => 0,
            Outcome::Replaced => 1,
            Outcome::Dropped => 2,
            Outcome::Stopped => 3,
            Outcome::Failed => 4,
        }
    }
}

pub fn headers() -> [&'static str; 5] {
    [
        "Video",
        tr_str("Esito", "Outcome"),
        tr_str("Prima", "Before"),
        tr_str("Dopo", "After"),
        tr_str("Risparmio", "Saved"),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResultSort {
    pub column: usize,
    pub ascending: bool,
}

/// The biggest saving first.
impl Default for ResultSort {
    fn default() -> Self {
        Self {
            column: 4,
            ascending: false,
        }
    }
}

impl ResultSort {
    pub fn toggle(&mut self, column: usize) {
        if self.column == column {
            self.ascending = !self.ascending;
        } else {
            self.column = column;
            self.ascending = true;
        }
    }
}

/// The space a file freed, only for a result that was kept.
pub fn saving(result: &ConversionResult) -> Option<SpaceChange> {
    match (result.success, result.original_size, result.new_size) {
        (true, Some(before), Some(after)) => Some(SpaceChange { before, after }),
        _ => None,
    }
}

/// The files that were kept, added up.
pub fn total(results: &[&ConversionResult]) -> Option<SpaceChange> {
    let mut space = SpaceChange::default();
    for change in results.iter().filter_map(|result| saving(result)) {
        space.add(change.before, change.after);
    }
    (space.before > 0).then_some(space)
}

fn name(result: &ConversionResult) -> String {
    result
        .source
        .file_name()
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|| result.source.display().to_string())
}

/// Bytes saved, negative when the file grew; files without a saving go last.
fn saved_bytes(result: &ConversionResult) -> i128 {
    match saving(result) {
        Some(change) => i128::from(change.before) - i128::from(change.after),
        None => i128::MIN,
    }
}

/// The rows in the order the table shows them.
pub fn sorted<'a>(results: &[&'a ConversionResult], sort: ResultSort) -> Vec<&'a ConversionResult> {
    let mut rows = results.to_vec();
    rows.sort_by(|a, b| {
        let ordering = match sort.column {
            0 => name(a).to_lowercase().cmp(&name(b).to_lowercase()),
            1 => Outcome::of(a).order().cmp(&Outcome::of(b).order()),
            2 => a.original_size.cmp(&b.original_size),
            3 => a.new_size.cmp(&b.new_size),
            _ => saved_bytes(a).cmp(&saved_bytes(b)),
        };
        if sort.ascending {
            ordering
        } else {
            ordering.reverse()
        }
    });
    rows
}

fn mono(ui: &mut egui::Ui, text: impl Into<String>, color: egui::Color32) {
    ui.label(RichText::new(text).font(theme::mono(11.0)).color(color));
}

/// Draw the table over the progress window. True once it is closed.
pub fn show(ctx: &egui::Context, results: &[&ConversionResult], sort: &mut ResultSort) -> bool {
    let mut closed = false;
    let rows = sorted(results, *sort);
    let modal = egui::Modal::new(egui::Id::unique("conversion-results"))
        .frame(crate::dialogs::modal_frame())
        .show(ctx, |ui| {
            let width = (ctx.content_rect().width() - 80.0).clamp(560.0, 820.0);
            ui.set_width(width);
            ui.label(
                RichText::new(tr_str("Risultati della conversione", "Conversion results"))
                    .size(theme::PAGE_TITLE_SIZE)
                    .strong(),
            );
            let kept = results.iter().filter(|result| result.success).count();
            ui.label(
                RichText::new(fotogramma_core::tr!(
                    "File: {} · convertiti: {}",
                    "Files: {} · converted: {}",
                    results.len(),
                    kept
                ))
                .color(theme::MUTED),
            );
            ui.add_space(6.0);
            let height = (ctx.content_rect().height() - 300.0).clamp(140.0, 420.0);
            ui.scope(|ui| {
                ui.style_mut().spacing.item_spacing = egui::vec2(6.0, 2.0);
                TableBuilder::new(ui)
                    .id_salt("conversion-results-table")
                    .striped(true)
                    .resizable(false)
                    .max_scroll_height(height)
                    .min_scrolled_height(height.min(28.0 * rows.len() as f32))
                    .cell_layout(egui::Layout::left_to_right(Align::Center))
                    .column(Column::remainder().at_least(200.0).clip(true))
                    .column(Column::exact(96.0))
                    .column(Column::exact(84.0))
                    .column(Column::exact(84.0))
                    .column(Column::exact(130.0))
                    .header(26.0, |mut header| {
                        for (index, title) in headers().iter().enumerate() {
                            header.col(|ui| {
                                let arrow = if sort.column == index {
                                    if sort.ascending {
                                        " ▲"
                                    } else {
                                        " ▼"
                                    }
                                } else {
                                    ""
                                };
                                let text =
                                    RichText::new(format!("{}{}", title.to_uppercase(), arrow))
                                        .font(theme::mono(10.0))
                                        .color(theme::LAVENDER);
                                if ui
                                    .add(egui::Button::new(text).frame(false))
                                    .on_hover_text(tr_str(
                                        "Ordina per questa colonna",
                                        "Sort by this column",
                                    ))
                                    .clicked()
                                {
                                    sort.toggle(index);
                                }
                            });
                        }
                    })
                    .body(|body| {
                        body.rows(26.0, rows.len(), |mut row| {
                            let result = rows[row.index()];
                            let outcome = Outcome::of(result);
                            row.col(|ui| {
                                ui.add(
                                    Label::new(RichText::new(name(result)).color(theme::TEXT))
                                        .truncate(),
                                );
                            });
                            row.col(|ui| {
                                ui.label(
                                    RichText::new(outcome.label())
                                        .small()
                                        .strong()
                                        .color(outcome.color()),
                                );
                            });
                            row.col(|ui| {
                                mono(ui, format_bytes(result.original_size), theme::MUTED);
                            });
                            row.col(|ui| {
                                let after = match outcome {
                                    Outcome::Converted | Outcome::Replaced => result.new_size,
                                    _ => None,
                                };
                                mono(ui, format_bytes(after), theme::MUTED);
                            });
                            row.col(|ui| match saving(result) {
                                Some(change) => mono(
                                    ui,
                                    format!(
                                        "{} ({})",
                                        format_bytes(Some(change.delta())),
                                        change.signed_percent()
                                    ),
                                    if change.is_saving() {
                                        theme::MINT
                                    } else {
                                        theme::CORAL_HOVER
                                    },
                                ),
                                None => mono(ui, "—", theme::MUTED),
                            });
                            let mut tooltip = result.source.display().to_string();
                            if result.success && result.destination != result.source {
                                tooltip.push_str("\n\n");
                                tooltip.push_str(&fotogramma_core::tr!(
                                    "Convertito in: {}",
                                    "Converted to: {}",
                                    result.destination.display()
                                ));
                            }
                            if !result.message.is_empty() {
                                tooltip.push_str("\n\n");
                                tooltip.push_str(&result.message);
                            }
                            row.response().on_hover_text(tooltip);
                        });
                    });
            });
            ui.add_space(8.0);
            if let Some(space) = total(results) {
                let (text, color) = if space.is_saving() {
                    (
                        fotogramma_core::tr!(
                            "Totale: da {} a {} · risparmiati {} ({})",
                            "Total: from {} to {} · {} saved ({})",
                            format_bytes(Some(space.before)),
                            format_bytes(Some(space.after)),
                            format_bytes(Some(space.delta())),
                            space.signed_percent()
                        ),
                        theme::MINT,
                    )
                } else {
                    (
                        fotogramma_core::tr!(
                            "Totale: da {} a {} · {} in più ({})",
                            "Total: from {} to {} · {} more ({})",
                            format_bytes(Some(space.before)),
                            format_bytes(Some(space.after)),
                            format_bytes(Some(space.delta())),
                            space.signed_percent()
                        ),
                        theme::CORAL_HOVER,
                    )
                };
                ui.label(RichText::new(text).strong().color(color));
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn result(name: &str, before: u64, after: Option<u64>, success: bool) -> ConversionResult {
        ConversionResult {
            source: PathBuf::from(format!("/in/{name}")),
            destination: PathBuf::from(format!("/out/{name}")),
            success,
            message: String::new(),
            cancelled: false,
            verified: false,
            replaced: false,
            original_size: Some(before),
            new_size: after,
            skipped: !success && after.is_some(),
        }
    }

    #[test]
    fn only_kept_files_count_towards_the_total() {
        let a = result("a.mkv", 2_000, Some(800), true);
        let b = result("b.mkv", 1_000, Some(1_100), true);
        // Dropped for a saving under the threshold: the original was kept.
        let c = result("c.mkv", 5_000, Some(4_900), false);
        let d = result("d.mkv", 9_000, None, false);
        let rows = [&a, &b, &c, &d];
        let space = total(&rows).unwrap();
        assert_eq!((space.before, space.after), (3_000, 1_900));
        assert_eq!(Outcome::of(&c), Outcome::Dropped);
        assert_eq!(Outcome::of(&d), Outcome::Failed);
        assert!(saving(&c).is_none());
    }

    #[test]
    fn the_biggest_saving_comes_first_and_files_without_one_last() {
        let a = result("a.mkv", 2_000, Some(1_800), true);
        let b = result("b.mkv", 2_000, Some(500), true);
        let c = result("c.mkv", 9_000, None, false);
        let d = result("d.mkv", 1_000, Some(1_200), true);
        let rows = [&a, &b, &c, &d];
        let names = |sort| {
            sorted(&rows, sort)
                .iter()
                .map(|result| name(result))
                .collect::<Vec<_>>()
        };
        assert_eq!(
            names(ResultSort::default()),
            ["b.mkv", "a.mkv", "d.mkv", "c.mkv"]
        );
        let mut by_name = ResultSort::default();
        by_name.toggle(0);
        assert_eq!(names(by_name), ["a.mkv", "b.mkv", "c.mkv", "d.mkv"]);
    }
}
