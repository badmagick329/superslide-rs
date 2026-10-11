//! The help overlay drawn with egui: a filter box over the bindings, grouped by section.

use egui::scroll_area::ScrollSource;
use egui::{
    Align, Align2, Color32, CornerRadius, Frame, Label, Layout, Margin, RichText, ScrollArea,
    TextEdit, vec2,
};

use crate::help::domain::{Binding, Help, Measured, Section, matching};

/// Share of the window the panel may cover, so the image stays in view at its edges.
const MAX_WIDTH_SHARE: f32 = 0.9;
const MAX_HEIGHT_SHARE: f32 = 0.8;
/// Wide enough for the longest description on one line at the usual window size; narrower
/// windows wrap descriptions instead.
const MAX_WIDTH: f32 = 760.0;
/// The keys column's share of the panel, and its widest.
const KEYS_SHARE: f32 = 0.35;
const MAX_KEYS_WIDTH: f32 = 220.0;
const MARGIN: i8 = 16;
const TEXT_SIZE: f32 = 16.0;
/// Height of the filter box, hint and margins above the list; the list gets the rest.
const CHROME_HEIGHT: f32 = 110.0;

/// Centred across the image, its top a tenth of the way down, so filtering shortens it without
/// moving the filter box. Notices draw in the foreground, over it. `hint` names the keys that
/// scroll and close it.
pub fn show(ctx: &egui::Context, help: &mut Help, rows: &[Binding], hint: &str) {
    let screen = ctx.content_rect();
    let width = (screen.width() * MAX_WIDTH_SHARE).min(MAX_WIDTH) - 2.0 * f32::from(MARGIN);
    let height = screen.height() * MAX_HEIGHT_SHARE;
    let list_height = (height - CHROME_HEIGHT).max(TEXT_SIZE * 2.0);
    egui::Area::new(egui::Id::new("help"))
        .anchor(
            Align2::CENTER_TOP,
            [0.0, screen.height() * (1.0 - MAX_HEIGHT_SHARE) / 2.0],
        )
        .fade_in(false)
        .show(ctx, |ui| {
            // An area offers its content only last frame's size, so a list shortened by the
            // filter could never grow back.
            ui.set_max_height(height);
            // Dark whatever the system theme, to match the notices and the error card.
            *ui.visuals_mut() = egui::Visuals::dark();
            Frame::new()
                // Opaque: over a bright image, any see-through backing leaves the text on a
                // tinted, uneven ground.
                .fill(Color32::from_gray(24))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(MARGIN))
                .show(ui, |ui| {
                    ui.set_width(width);
                    filter_box(ui, help);
                    ui.add(
                        Label::new(
                            RichText::new(hint)
                                .size(TEXT_SIZE - 2.0)
                                .color(Color32::GRAY),
                        )
                        .wrap(),
                    );
                    ui.add_space(8.0);
                    list(ui, help, rows, list_height);
                });
        });
}

fn filter_box(ui: &mut egui::Ui, help: &mut Help) {
    let response = ui.add(
        TextEdit::singleline(&mut help.filter)
            .hint_text("Press / to filter")
            .font(egui::FontId::proportional(TEXT_SIZE))
            .desired_width(f32::INFINITY),
    );
    // After the box has drawn, so the `/` that asked for focus is not typed into it.
    if std::mem::take(&mut help.focus_filter) {
        response.request_focus();
    }
    // egui drops focus on Esc before the box sees the frame; Esc in the box also clears it.
    if response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Escape)) {
        help.filter.clear();
    }
}

fn list(ui: &mut egui::Ui, help: &mut Help, rows: &[Binding], height: f32) {
    let mut area = ScrollArea::vertical()
        .max_height(height)
        .auto_shrink([false, true])
        // The shell turns the wheel into scrolls, wherever the cursor is; egui scrolling too would
        // double them under the panel.
        .scroll_source(ScrollSource::SCROLL_BAR);
    if let Some(offset) = help.take_scroll() {
        area = area.vertical_scroll_offset(offset);
    }
    let line = TEXT_SIZE + ui.spacing().item_spacing.y + 4.0;
    let keys_width = (ui.available_width() * KEYS_SHARE).min(MAX_KEYS_WIDTH);
    let output = area.show(ui, |ui| {
        let rows = matching(rows, &help.filter);
        if rows.is_empty() {
            ui.label(
                RichText::new("No bindings match")
                    .size(TEXT_SIZE)
                    .color(Color32::GRAY),
            );
        }
        let mut section = None;
        for row in rows {
            if section != Some(row.section) {
                if section.is_some() {
                    ui.add_space(8.0);
                }
                section = Some(row.section);
                ui.label(
                    RichText::new(title(row.section))
                        .size(TEXT_SIZE)
                        .strong()
                        .color(Color32::from_rgb(140, 180, 255)),
                );
            }
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    vec2(keys_width, 0.0),
                    Layout::top_down(Align::Min),
                    |ui| {
                        ui.set_width(keys_width);
                        ui.add(
                            Label::new(
                                RichText::new(row.keys)
                                    .size(TEXT_SIZE)
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .wrap(),
                        );
                    },
                );
                // Wraps within the rest of the row rather than widening the panel.
                ui.add(
                    Label::new(
                        RichText::new(row.action)
                            .size(TEXT_SIZE)
                            .color(Color32::LIGHT_GRAY),
                    )
                    .wrap(),
                );
            });
        }
    });
    help.measured = Measured {
        offset: output.state.offset.y,
        max_offset: (output.content_size.y - output.inner_rect.height()).max(0.0),
        page: output.inner_rect.height(),
        line,
    };
}

fn title(section: Section) -> &'static str {
    match section {
        Section::Navigate => "Navigate",
        Section::View => "View",
        Section::Order => "Order",
        Section::Marks => "Marks",
        Section::Files => "Files",
        Section::Clipboard => "Clipboard",
        Section::Window => "Window",
    }
}

/// A Vim count or half-typed key, in the bottom-right corner as Vim shows it, until the binding
/// completes or Esc clears it. The info overlay has the bottom-left, notices the top-right.
pub fn show_pending(ctx: &egui::Context, keys: &str) {
    egui::Area::new(egui::Id::new("pending keys"))
        .anchor(Align2::RIGHT_BOTTOM, [-12.0, -12.0])
        .order(egui::Order::Foreground)
        .interactable(false)
        .fade_in(false)
        .show(ctx, |ui| {
            Frame::new()
                .fill(Color32::from_black_alpha(240))
                .corner_radius(CornerRadius::same(4))
                .inner_margin(Margin::symmetric(8, 4))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(keys)
                            .size(TEXT_SIZE + 4.0)
                            .monospace()
                            .color(Color32::WHITE),
                    );
                });
        });
}
