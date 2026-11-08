//! Virtualized results table with badges, sorting, filtering and selection.

use eframe::egui::{self, Align, Align2, CornerRadius, FontId, Label, RichText, Sense, Stroke};
use egui_extras::{Column, TableBuilder};
use fotogramma_core::formatting::{codec_label, format_bytes, format_duration};
use fotogramma_core::i18n::tr_str;
use fotogramma_core::scanner::{Category, VideoInfo};

use crate::theme;

pub fn headers() -> [&'static str; 6] {
    [
        "Video",
        "Codec",
        tr_str("Durata", "Duration"),
        tr_str("Risoluzione", "Resolution"),
        tr_str("Dimensione", "Size"),
        tr_str("Azione", "Action"),
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FilterCategory {
    All,
    Convert,
    Compatible,
    Error,
}

impl FilterCategory {
    pub fn matches(self, category: Category) -> bool {
        match self {
            FilterCategory::All => true,
            FilterCategory::Convert => category == Category::Convert,
            FilterCategory::Compatible => category == Category::Compatible,
            FilterCategory::Error => category == Category::Error,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortState {
    pub column: usize,
    pub ascending: bool,
}

impl Default for SortState {
    fn default() -> Self {
        Self {
            column: 0,
            ascending: true,
        }
    }
}

impl SortState {
    pub fn toggle(&mut self, column: usize) {
        if self.column == column {
            self.ascending = !self.ascending;
        } else {
            self.column = column;
            self.ascending = true;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RowClick {
    pub index: usize,
    pub modifiers: egui::Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TableAction {
    Click(RowClick),
    Open(usize),
}

pub fn visible_indices(
    videos: &[VideoInfo],
    category: FilterCategory,
    query: &str,
    sort: SortState,
) -> Vec<usize> {
    let query = query.trim().to_lowercase();
    let mut indices: Vec<usize> = videos
        .iter()
        .enumerate()
        .filter(|(_, video)| category.matches(video.category))
        .filter(|(_, video)| {
            if query.is_empty() {
                return true;
            }
            let haystack = format!(
                "{} {} {}",
                video.name,
                video.relative_path,
                video.codec.as_deref().unwrap_or("")
            )
            .to_lowercase();
            haystack.contains(&query)
        })
        .map(|(index, _)| index)
        .collect();

    indices.sort_by(|left, right| {
        let a = &videos[*left];
        let b = &videos[*right];
        let ordering = match sort.column {
            0 => a
                .relative_path
                .to_lowercase()
                .cmp(&b.relative_path.to_lowercase()),
            1 => a
                .codec
                .as_deref()
                .unwrap_or("")
                .cmp(b.codec.as_deref().unwrap_or("")),
            2 => a
                .duration
                .unwrap_or(-1.0)
                .total_cmp(&b.duration.unwrap_or(-1.0)),
            3 => resolution(a).cmp(&resolution(b)),
            4 => a.size.unwrap_or(0).cmp(&b.size.unwrap_or(0)),
            _ => a.category.order().cmp(&b.category.order()),
        };
        if sort.ascending {
            ordering
        } else {
            ordering.reverse()
        }
    });
    indices
}

fn resolution(video: &VideoInfo) -> u64 {
    u64::from(video.width.unwrap_or(0)) * u64::from(video.height.unwrap_or(0))
}

pub fn show(
    ui: &mut egui::Ui,
    videos: &[VideoInfo],
    visible: &[usize],
    selected: &std::collections::HashSet<std::path::PathBuf>,
    converted: &std::collections::HashMap<std::path::PathBuf, std::path::PathBuf>,
    sort: &mut SortState,
) -> Option<TableAction> {
    let mut action = None;
    ui.scope(|ui| {
        ui.style_mut().spacing.item_spacing = egui::vec2(6.0, 2.0);
        TableBuilder::new(ui)
            .striped(true)
            .resizable(false)
            .sense(Sense::click())
            .cell_layout(egui::Layout::left_to_right(Align::Center))
            .column(Column::remainder().at_least(240.0).clip(true))
            .column(Column::exact(76.0))
            .column(Column::exact(70.0))
            .column(Column::exact(116.0))
            .column(Column::exact(92.0))
            .column(Column::exact(156.0))
            .header(28.0, |mut header| {
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
                        let text = RichText::new(format!("{}{}", title.to_uppercase(), arrow))
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
                body.rows(50.0, visible.len(), |mut row| {
                    let video_index = visible[row.index()];
                    let video = &videos[video_index];
                    let is_selected = selected.contains(&video.path);
                    row.set_selected(is_selected);
                    row.col(|ui| {
                        ui.horizontal(|ui| {
                            badge(ui, &video.extension);
                            ui.vertical(|ui| {
                                ui.add(
                                    Label::new(
                                        RichText::new(&video.name).strong().color(theme::TEXT),
                                    )
                                    .truncate(),
                                );
                                ui.add(
                                    Label::new(
                                        RichText::new(video.folder_label())
                                            .small()
                                            .color(theme::MUTED),
                                    )
                                    .truncate(),
                                );
                            });
                        });
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(codec_label(video.codec.as_deref()))
                                .font(theme::mono(11.0))
                                .color(theme::TEXT),
                        );
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(format_duration(video.duration))
                                .font(theme::mono(11.0))
                                .color(theme::MUTED),
                        );
                    });
                    row.col(|ui| {
                        let text = match (video.width, video.height) {
                            (Some(width), Some(height)) if width > 0 && height > 0 => {
                                format!("{width} × {height}")
                            }
                            _ => "—".to_string(),
                        };
                        ui.label(
                            RichText::new(text)
                                .font(theme::mono(11.0))
                                .color(theme::MUTED),
                        );
                    });
                    row.col(|ui| {
                        ui.label(
                            RichText::new(format_bytes(video.size))
                                .font(theme::mono(11.0))
                                .color(theme::MUTED),
                        );
                    });
                    row.col(|ui| {
                        let color = match video.category {
                            Category::Convert => theme::CORAL,
                            Category::Compatible => theme::MINT,
                            Category::Error => theme::ERROR,
                        };
                        if converted.contains_key(&video.path) {
                            ui.label(
                                RichText::new(tr_str("Già convertito", "Already converted"))
                                    .strong()
                                    .color(theme::LAVENDER),
                            );
                        } else {
                            ui.label(RichText::new(video.status_label()).strong().color(color));
                        }
                    });

                    let response = row.response();
                    let mut tooltip = match &video.error {
                        Some(error) => format!("{}\n\n{error}", video.path.display()),
                        None => video.path.display().to_string(),
                    };
                    if let Some(output) = converted.get(&video.path) {
                        tooltip.push_str("\n\n");
                        tooltip.push_str(&if *output == video.path {
                            tr_str(
                                "Originale sostituito dopo verifica",
                                "Original replaced after verification",
                            )
                            .to_string()
                        } else {
                            fotogramma_core::tr!(
                                "Convertito in: {}",
                                "Converted to: {}",
                                output.display()
                            )
                        });
                    }
                    let response = response.on_hover_text(tooltip);
                    if response.double_clicked() {
                        action = Some(TableAction::Open(video_index));
                    } else if response.clicked() {
                        action = Some(TableAction::Click(RowClick {
                            index: video_index,
                            modifiers: response.ctx.input(|input| input.modifiers),
                        }));
                    }
                });
            });
    });
    action
}

fn badge(ui: &mut egui::Ui, extension: &str) {
    let (rect, _response) = ui.allocate_exact_size(egui::vec2(46.0, 30.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(rect, CornerRadius::same(6), theme::BADGE_BG);
    painter.rect_stroke(
        rect,
        CornerRadius::same(6),
        Stroke::new(1.0, theme::LINE),
        egui::StrokeKind::Inside,
    );
    let label = if extension.is_empty() {
        "VIDEO".to_string()
    } else {
        extension.to_uppercase().chars().take(6).collect()
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        FontId::monospace(9.0),
        theme::BADGE_TEXT,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use fotogramma_core::scanner::Category;
    use std::path::PathBuf;

    fn video(name: &str, codec: &str, category: Category, size: u64) -> VideoInfo {
        VideoInfo {
            hdr: false,
            path: PathBuf::from(format!("/videos/{name}")),
            relative_path: format!("Vacanze/{name}"),
            name: name.to_string(),
            folder: "Vacanze".into(),
            extension: "mkv".into(),
            category,
            codec: Some(codec.into()),
            size: Some(size),
            duration: Some(10.0),
            width: Some(1920),
            height: Some(1080),
            error: None,
        }
    }

    #[test]
    fn filters_by_category_and_query() {
        let videos = vec![
            video("Tramonto.mkv", "h264", Category::Convert, 10),
            video("Gita.mkv", "hevc", Category::Compatible, 20),
            video("Rotto.mkv", "h264", Category::Error, 30),
        ];
        let all = visible_indices(&videos, FilterCategory::All, "", SortState::default());
        assert_eq!(all.len(), 3);
        let convert = visible_indices(&videos, FilterCategory::Convert, "", SortState::default());
        assert_eq!(convert, vec![0]);
        let searched = visible_indices(&videos, FilterCategory::All, "gita", SortState::default());
        assert_eq!(searched, vec![1]);
    }

    #[test]
    fn sorts_by_size_descending() {
        let videos = vec![
            video("a.mkv", "h264", Category::Convert, 10),
            video("b.mkv", "h264", Category::Convert, 30),
            video("c.mkv", "h264", Category::Convert, 20),
        ];
        let indices = visible_indices(
            &videos,
            FilterCategory::All,
            "",
            SortState {
                column: 4,
                ascending: false,
            },
        );
        assert_eq!(indices, vec![1, 2, 0]);
    }

    #[test]
    fn toggling_a_column_reverses_it() {
        let mut sort = SortState::default();
        sort.toggle(3);
        assert_eq!(sort.column, 3);
        assert!(sort.ascending);
        sort.toggle(3);
        assert!(!sort.ascending);
        sort.toggle(1);
        assert_eq!(sort.column, 1);
        assert!(sort.ascending);
    }
}
