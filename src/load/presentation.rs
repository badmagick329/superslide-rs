//! The error card: what is drawn in place of an image that failed to decode.

use egui::{Align2, Color32, CornerRadius, Frame, Margin, RichText};

/// Centred, so it cannot be mistaken for a notice, which sits top-right and fades.
pub fn error_card(ctx: &egui::Context, file_name: &str, reason: &str) {
    egui::Area::new(egui::Id::new("error card"))
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        // Clicks go through to the viewer, so clicking past a broken image still navigates.
        .interactable(false)
        .fade_in(false)
        .show(ctx, |ui| {
            Frame::new()
                .fill(Color32::from_gray(40))
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::symmetric(24, 16))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(RichText::new(file_name).size(22.0).color(Color32::WHITE));
                        ui.label(RichText::new(reason).size(18.0).color(Color32::LIGHT_GRAY));
                    });
                });
        });
}
