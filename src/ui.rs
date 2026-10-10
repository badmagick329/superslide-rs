//! egui content drawn over the image: info overlay, notices and the preferences screen.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, CornerRadius, Frame, Margin, RichText};

/// What the info overlay shows; I cycles through these in order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(
    dead_code,
    reason = "scaffold: the info overlay arrives with its issue"
)]
pub enum InfoOverlay {
    #[default]
    Hidden,
    FileName,
    Full,
}

/// Fully visible this long, then fading out over `NOTICE_FADE`.
const NOTICE_SHOWN: Duration = Duration::from_secs(3);
const NOTICE_FADE: Duration = Duration::from_millis(500);
/// More than this and the oldest goes; a burst of errors must not cover the image.
const MAX_NOTICES: usize = 3;
/// Gap from the window corner and between notices, in points.
const NOTICE_SPACING: f32 = 12.0;

/// Short texts over the image, top-right, oldest on top. The top-left corner is left to the info
/// overlay.
#[derive(Debug, Default)]
pub struct Notices {
    /// Oldest first.
    shown: VecDeque<Notice>,
}

#[derive(Debug)]
struct Notice {
    text: String,
    since: Instant,
}

impl Notices {
    /// Repeating a text already shown restarts its timer and moves it to the bottom, so holding a
    /// key that fails does not stack copies.
    pub fn notify(&mut self, text: impl Into<String>, now: Instant) {
        let text = text.into();
        self.shown.retain(|notice| notice.text != text);
        if self.shown.len() == MAX_NOTICES {
            self.shown.pop_front();
        }
        self.shown.push_back(Notice { text, since: now });
    }

    /// When the notices next look different: `now` while one is fading (redraw every frame),
    /// else when the next fade starts. `None` when nothing is shown.
    pub fn next_change(&self, now: Instant) -> Option<Instant> {
        self.shown
            .iter()
            .map(|notice| (notice.since + NOTICE_SHOWN).max(now))
            .min()
    }

    /// Drops expired notices and draws the rest.
    pub fn show(&mut self, ctx: &egui::Context, now: Instant) {
        self.shown
            .retain(|notice| now < notice.since + NOTICE_SHOWN + NOTICE_FADE);
        if self.shown.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("notices"))
            .anchor(Align2::RIGHT_TOP, [-NOTICE_SPACING, NOTICE_SPACING])
            // Never takes clicks or focus: input under a notice still reaches the viewer.
            .interactable(false)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = NOTICE_SPACING / 2.0;
                ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    for notice in &self.shown {
                        ui.scope(|ui| {
                            ui.multiply_opacity(opacity(notice.since, now));
                            // Its own dark backing keeps white text legible on any image. The
                            // surface is sRGB, so alpha blends in linear light: 200 left white
                            // showing through as mid-grey; 240 darkens it to about sRGB 70.
                            Frame::new()
                                .fill(Color32::from_black_alpha(240))
                                .corner_radius(CornerRadius::same(6))
                                .inner_margin(Margin::symmetric(12, 8))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(&notice.text)
                                            .size(20.0)
                                            .color(Color32::WHITE),
                                    );
                                });
                        });
                    }
                });
            });
    }
}

fn opacity(since: Instant, now: Instant) -> f32 {
    let fading = now.saturating_duration_since(since + NOTICE_SHOWN);
    1.0 - (fading.as_secs_f32() / NOTICE_FADE.as_secs_f32()).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(notices: &Notices) -> Vec<&str> {
        notices.shown.iter().map(|n| n.text.as_str()).collect()
    }

    #[test]
    fn repeating_a_notice_restarts_it_instead_of_stacking() {
        let start = Instant::now();
        let mut notices = Notices::default();
        notices.notify("a", start);
        notices.notify("b", start);
        notices.notify("a", start + Duration::from_secs(2));
        assert_eq!(texts(&notices), ["b", "a"]);
        // "b" starts fading first; "a" was restarted.
        assert_eq!(
            notices.next_change(start + Duration::from_secs(1)),
            Some(start + NOTICE_SHOWN)
        );
        notices.shown.pop_front();
        assert_eq!(
            notices.next_change(start + Duration::from_secs(2)),
            Some(start + Duration::from_secs(2) + NOTICE_SHOWN)
        );
    }

    #[test]
    fn a_fourth_notice_pushes_out_the_oldest() {
        let now = Instant::now();
        let mut notices = Notices::default();
        for text in ["a", "b", "c", "d"] {
            notices.notify(text, now);
        }
        assert_eq!(texts(&notices), ["b", "c", "d"]);
    }

    #[test]
    fn notices_hold_then_fade_out() {
        let start = Instant::now();
        assert_eq!(opacity(start, start), 1.0);
        assert_eq!(opacity(start, start + NOTICE_SHOWN), 1.0);
        let halfway = start + NOTICE_SHOWN + NOTICE_FADE / 2;
        assert!((opacity(start, halfway) - 0.5).abs() < 1e-3);
        assert_eq!(opacity(start, start + NOTICE_SHOWN + NOTICE_FADE), 0.0);

        let mut notices = Notices::default();
        notices.notify("a", start);
        // While fading, every frame changes.
        assert_eq!(notices.next_change(halfway), Some(halfway));
    }
}
