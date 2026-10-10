//! The info overlay drawn with egui, bottom-left.

use egui::{Align2, Color32, CornerRadius, Frame, Label, Margin, RichText};

use crate::info_overlay::domain::{Details, InfoOverlay, lines};

/// Gap from the window corner, as the notices keep from theirs.
const SPACING: f32 = 12.0;
const TEXT_SIZE: f32 = 16.0;
/// Share of the window a line may take before it wraps, so a long folder path never reaches the
/// notices in the opposite corner.
const MAX_WIDTH_SHARE: f32 = 0.5;

/// Redrawn from `details` every frame, so it changes on the frame the image or the view does.
pub fn show(ctx: &egui::Context, mode: InfoOverlay, details: &Details) {
    let lines = lines(mode, details);
    let Some((title, rest)) = lines.split_first() else {
        return;
    };
    let max_width = ctx.content_rect().width() * MAX_WIDTH_SHARE;
    egui::Area::new(egui::Id::new("info overlay"))
        .anchor(Align2::LEFT_BOTTOM, [SPACING, -SPACING])
        // Under every other panel: the help overlay can reach this corner in a small window.
        .order(egui::Order::Background)
        // Never takes clicks or focus: input over it still reaches the viewer.
        .interactable(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.set_max_width(max_width);
            // The notices' backing, for the same reason: legible white text on any image.
            Frame::new()
                .fill(Color32::from_black_alpha(240))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    let line = |ui: &mut egui::Ui, text: RichText| {
                        ui.add(Label::new(text.size(TEXT_SIZE)).wrap());
                    };
                    line(ui, RichText::new(title).strong().color(Color32::WHITE));
                    for text in rest {
                        line(ui, RichText::new(text).color(Color32::LIGHT_GRAY));
                    }
                });
        });
}
