//! The preferences screen drawn with egui, and the notices the preferences and session files give.

use std::io;

use egui::{
    Align2, Color32, CornerRadius, DragValue, Frame, Grid, Label, Margin, RichText, ScrollArea,
    TextEdit,
};

use crate::info_overlay::domain::InfoOverlay;
use crate::prefs::domain::session::Session;
use crate::prefs::domain::{
    ADVANCE_LIMIT_MS, PRELOAD_BUDGET_MB, Preferences, SLIDESHOW_INTERVAL_S, Screen, folder_filter,
};
use crate::prefs::infrastructure::{LoadProblem, Stored};
use crate::view::domain::MagnificationFilter;

/// The help overlay's width; taller than it, so every row fits an 800 × 600 window at 125 %
/// scaling without scrolling.
const MAX_WIDTH_SHARE: f32 = 0.9;
const MAX_HEIGHT_SHARE: f32 = 0.9;
const MAX_WIDTH: f32 = 640.0;
const MARGIN: i8 = 16;
const TEXT_SIZE: f32 = 16.0;
const NOTE_SIZE: f32 = 14.0;
/// Height of the title, hint, button and margins around the rows; the rows get the rest.
const CHROME_HEIGHT: f32 = 120.0;
const LABEL_SHARE: f32 = 0.3;
const COLUMN_GAP: f32 = 16.0;

pub fn load_problem(problem: &LoadProblem) -> String {
    match problem {
        LoadProblem::Damaged => format!(
            "Preferences were unreadable and are reset; the old file is {}.bak",
            Preferences::FILE_NAME
        ),
        LoadProblem::DamagedNotKept(reason) => {
            format!("Preferences were unreadable and are reset: {reason}")
        }
        LoadProblem::Unreadable(reason) => {
            format!("Cannot read preferences, so defaults apply: {reason}")
        }
    }
}

pub fn session_problem(problem: &LoadProblem) -> String {
    match problem {
        LoadProblem::Damaged => format!(
            "The last session was unreadable and is not restored; the old file is {}.bak",
            Session::FILE_NAME
        ),
        LoadProblem::DamagedNotKept(reason) => {
            format!("The last session was unreadable and is not restored: {reason}")
        }
        LoadProblem::Unreadable(reason) => {
            format!("Cannot read the last session, so it is not restored: {reason}")
        }
    }
}

pub fn save_failed(error: &io::Error) -> String {
    format!("Cannot save preferences: {error}")
}

pub fn session_save_failed(error: &io::Error) -> String {
    format!("Cannot save the session: {error}")
}

/// Centred across the image, its top a twentieth of the way down. Every
/// change lands in `preferences` on the frame it is made; the caller applies it.
pub fn show(ctx: &egui::Context, preferences: &mut Preferences, screen: &mut Screen) {
    let window = ctx.content_rect();
    let width = (window.width() * MAX_WIDTH_SHARE).min(MAX_WIDTH) - 2.0 * f32::from(MARGIN);
    let height = window.height() * MAX_HEIGHT_SHARE;
    egui::Area::new(egui::Id::new("preferences"))
        .anchor(
            Align2::CENTER_TOP,
            [0.0, window.height() * (1.0 - MAX_HEIGHT_SHARE) / 2.0],
        )
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_max_height(height);
            // Dark whatever the system theme, to match the help overlay and the notices.
            *ui.visuals_mut() = egui::Visuals::dark();
            // Solid, not egui's floating bar that shows only on hover: in a small window the bar
            // is the only sign that more rows lie below.
            ui.style_mut().spacing.scroll = egui::style::ScrollStyle::solid();
            ui.style_mut().override_text_style = Some(egui::TextStyle::Body);
            ui.style_mut()
                .text_styles
                .insert(egui::TextStyle::Body, egui::FontId::proportional(TEXT_SIZE));
            ui.style_mut().text_styles.insert(
                egui::TextStyle::Button,
                egui::FontId::proportional(TEXT_SIZE),
            );
            Frame::new()
                // Opaque, as the help overlay is: a see-through backing tints text unevenly.
                .fill(Color32::from_gray(24))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(MARGIN))
                .show(ui, |ui| {
                    ui.set_width(width);
                    ui.label(RichText::new("Preferences").strong().color(Color32::WHITE));
                    note(
                        ui,
                        "Changes apply at once and are saved when this closes · P or Esc closes",
                    );
                    ui.add_space(8.0);
                    ScrollArea::vertical()
                        .max_height((height - CHROME_HEIGHT).max(TEXT_SIZE * 4.0))
                        .auto_shrink([false, true])
                        .show(ui, |ui| rows(ui, preferences, screen, width));
                    ui.add_space(8.0);
                    if ui.button("Reset to defaults").clicked() {
                        *preferences = Preferences::default();
                        *screen = Screen::new(preferences);
                    }
                });
        });
}

fn rows(ui: &mut egui::Ui, preferences: &mut Preferences, screen: &mut Screen, width: f32) {
    let label_width = width * LABEL_SHARE;
    let value_width = width - label_width - COLUMN_GAP;
    Grid::new("preference rows")
        .num_columns(2)
        .min_col_width(label_width)
        .max_col_width(value_width)
        .spacing([COLUMN_GAP, 6.0])
        .show(ui, |ui| {
            row(ui, "Advance limit", "Held Space or Backspace", |ui| {
                ui.add(
                    DragValue::new(&mut preferences.advance_limit_ms)
                        .range(ADVANCE_LIMIT_MS)
                        .speed(10)
                        .suffix(" ms"),
                );
            });
            row(ui, "Slideshow interval", "", |ui| {
                ui.add(
                    DragValue::new(&mut preferences.slideshow_interval_s)
                        .range(SLIDESHOW_INTERVAL_S)
                        .suffix(" s"),
                );
            });
            top_label(ui, "Folder filter");
            ui.vertical(|ui| {
                let edited = ui
                    .add(
                        TextEdit::multiline(&mut screen.folder_filter)
                            // Grows a line at a time as names are added.
                            .desired_rows(1)
                            .desired_width(value_width),
                    )
                    .changed();
                if edited {
                    preferences.folder_filter = folder_filter(&screen.folder_filter);
                }
                note(ui, "One per line; applies at the next open");
            });
            ui.end_row();
            row(ui, "Preload budget", "GPU memory for nearby images", |ui| {
                ui.add(
                    DragValue::new(&mut preferences.preload_budget_mb)
                        .range(PRELOAD_BUDGET_MB)
                        .speed(16)
                        .suffix(" MB"),
                );
            });
            row(ui, "Start fullscreen", "Applies at the next start", |ui| {
                ui.checkbox(&mut preferences.start_fullscreen, "");
            });
            row(
                ui,
                "Session restore",
                "Reopens the last image at the next start",
                |ui| {
                    ui.checkbox(&mut preferences.session_restore, "");
                },
            );
            row(ui, "Magnification", "Ctrl+F", |ui| {
                let filter = &mut preferences.magnification_filter;
                ui.radio_value(filter, MagnificationFilter::Linear, "Smooth");
                ui.radio_value(filter, MagnificationFilter::Nearest, "Sharp pixels");
            });
            row(ui, "Info overlay", "I", |ui| {
                let mode = &mut preferences.info_overlay;
                ui.radio_value(mode, InfoOverlay::Hidden, "Hidden");
                ui.radio_value(mode, InfoOverlay::FileName, "File name");
                ui.radio_value(mode, InfoOverlay::Full, "Full");
            });
        });
}

/// A label, then its widgets with an optional note beside them. One line per row lets every row
/// fit an 800 × 600 window, so notes stay short.
fn row(ui: &mut egui::Ui, text: &str, hint: &str, widgets: impl FnOnce(&mut egui::Ui)) {
    ui.label(RichText::new(text).color(Color32::WHITE));
    ui.horizontal(|ui| {
        widgets(ui);
        if !hint.is_empty() {
            ui.add_space(8.0);
            note(ui, hint);
        }
    });
    ui.end_row();
}

/// Top-aligned, so the label stays level with the first line of a tall widget.
fn top_label(ui: &mut egui::Ui, text: &str) {
    ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
        ui.label(RichText::new(text).color(Color32::WHITE));
    });
}

fn note(ui: &mut egui::Ui, text: &str) {
    ui.add(Label::new(RichText::new(text).size(NOTE_SIZE).color(Color32::GRAY)).wrap());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_damaged_notices_name_the_backups() {
        assert_eq!(
            load_problem(&LoadProblem::Damaged),
            "Preferences were unreadable and are reset; the old file is superslide-prefs.toml.bak"
        );
        assert!(session_problem(&LoadProblem::Damaged).ends_with("superslide-session.toml.bak"));
    }
}
