//! How the image list's order is worded in the notice each sort key shows, and the corner that
//! shows the marks.

use egui::{Align2, Color32, CornerRadius, Frame, Label, Margin, RichText};

use crate::browse::domain::{Order, SortKey, SortMode};

/// What the marks corner shows; drawn only while any image is marked.
pub struct Marks {
    pub count: usize,
    /// The image on screen is one of them.
    pub on_screen: bool,
}

/// Top-left, the one corner nothing else uses: notices have the top-right, the info overlay the
/// bottom-left, pending keys the bottom-right. The count is always in sight, so a bulk action
/// never surprises the owner with images they forgot were marked.
pub fn show_marks(ctx: &egui::Context, marks: &Marks) {
    egui::Area::new(egui::Id::new("marks"))
        .anchor(Align2::LEFT_TOP, [12.0, 12.0])
        .order(egui::Order::Background)
        // Never takes clicks or focus: input over it still reaches the viewer.
        .interactable(false)
        .fade_in(false)
        .show(ctx, |ui| {
            // The notices' backing, for legible text on any image.
            Frame::new()
                .fill(Color32::from_black_alpha(240))
                .corner_radius(CornerRadius::same(6))
                .inner_margin(Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 2.0;
                    // Never wrapped: an area keeps last frame's width, so after a frame with
                    // only the narrower count line, `✔ Marked` wrapped to fit it.
                    if marks.on_screen {
                        let text = RichText::new("\u{2714} Marked")
                            .size(20.0)
                            .strong()
                            .color(Color32::GOLD);
                        ui.add(Label::new(text).extend());
                    }
                    let text = RichText::new(format!("{} marked", marks.count))
                        .size(16.0)
                        .color(Color32::LIGHT_GRAY);
                    ui.add(Label::new(text).extend());
                });
        });
}

/// Names the whole order, not just the key pressed, so the notice also says what stayed on.
pub fn describe(order: Order) -> String {
    let mut text = match order.mode {
        SortMode::By(SortKey::Name) => "Sorted by name",
        SortMode::By(SortKey::Date) => "Sorted by date",
        SortMode::By(SortKey::Size) => "Sorted by size",
        SortMode::Shuffle { .. } => "Shuffled",
    }
    .to_owned();
    if order.reversed {
        text += ", reversed";
    }
    if order.grouped {
        text += ", grouped by folder";
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_notice_names_every_part_of_the_order() {
        let date = Order::new(SortMode::By(SortKey::Date));
        assert_eq!(describe(date), "Sorted by date");
        let all = Order {
            reversed: true,
            grouped: true,
            ..date
        };
        assert_eq!(describe(all), "Sorted by date, reversed, grouped by folder");
        assert_eq!(
            describe(Order::new(SortMode::Shuffle { seed: 1 })),
            "Shuffled"
        );
    }
}
