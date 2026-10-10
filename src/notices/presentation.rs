//! Notices drawn with egui, and the wording of the counts they carry.

use std::time::Instant;

use egui::{Align2, Color32, CornerRadius, Frame, Margin, RichText};

use crate::notices::domain::Notices;

/// Gap from the window corner and between notices, in points.
const NOTICE_SPACING: f32 = 12.0;

/// Drops expired notices and draws the rest, top-right, oldest on top. The top-left corner is left
/// to the info overlay.
pub fn show(notices: &mut Notices, ctx: &egui::Context, now: Instant) {
    let visible = notices.visible(now);
    if visible.is_empty() {
        return;
    }
    egui::Area::new(egui::Id::new("notices"))
        .anchor(Align2::RIGHT_TOP, [-NOTICE_SPACING, NOTICE_SPACING])
        // Over the help overlay, which can reach the corner in a small window.
        .order(egui::Order::Foreground)
        // Never takes clicks or focus: input under a notice still reaches the viewer.
        .interactable(false)
        .fade_in(false)
        .show(ctx, |ui| {
            ui.spacing_mut().item_spacing.y = NOTICE_SPACING / 2.0;
            ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                for (text, opacity) in visible {
                    ui.scope(|ui| {
                        ui.multiply_opacity(opacity);
                        // Its own dark backing keeps white text legible on any image. The
                        // surface is sRGB, so alpha blends in linear light: 200 left white
                        // showing through as mid-grey; 240 darkens it to about sRGB 70.
                        Frame::new()
                            .fill(Color32::from_black_alpha(240))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.label(RichText::new(text).size(20.0).color(Color32::WHITE));
                            });
                    });
                }
            });
        });
}

/// `12345` as `12,345`, for counts in notices.
pub fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut text = String::with_capacity(digits.len() * 4 / 3);
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            text.push(',');
        }
        text.push(digit);
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thousands_are_separated() {
        assert_eq!(thousands(0), "0");
        assert_eq!(thousands(999), "999");
        assert_eq!(thousands(1000), "1,000");
        assert_eq!(thousands(12345), "12,345");
        assert_eq!(thousands(1234567), "1,234,567");
    }
}
