//! Which notices show and how opaque each is: timing, replacement and the cap. No drawing.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Fully visible this long, then fading out over `NOTICE_FADE`.
const NOTICE_SHOWN: Duration = Duration::from_secs(3);
const NOTICE_FADE: Duration = Duration::from_millis(500);
/// More than this and the oldest goes; a burst of errors must not cover the image.
const MAX_NOTICES: usize = 3;

/// The notices on screen. `notices::presentation` draws them top-right, oldest on top.
#[derive(Debug, Default)]
pub struct Notices {
    /// Oldest first.
    shown: VecDeque<Notice>,
}

#[derive(Debug)]
struct Notice {
    /// Notices with the same key replace each other; for most, the key is the text.
    key: String,
    text: String,
    since: Instant,
}

impl Notices {
    /// Repeating a text already shown restarts its timer and moves it to the bottom, so holding a
    /// key that fails does not stack copies.
    pub fn notify(&mut self, text: impl Into<String>, now: Instant) {
        let text = text.into();
        self.notify_as(text.clone(), text, now);
    }

    /// Replaces the notice shown under `key`, if any, and moves it to the bottom: for a text that
    /// changes, such as a progress count.
    pub fn notify_as(&mut self, key: impl Into<String>, text: impl Into<String>, now: Instant) {
        let key = key.into();
        self.shown.retain(|notice| notice.key != key);
        if self.shown.len() == MAX_NOTICES {
            self.shown.pop_front();
        }
        self.shown.push_back(Notice {
            key,
            text: text.into(),
            since: now,
        });
    }

    /// Takes down the notice shown under `key` at once, for one whose news is overtaken, such as
    /// `Loading…` once the image is on screen.
    pub fn dismiss(&mut self, key: &str) {
        self.shown.retain(|notice| notice.key != key);
    }

    /// When the notices next look different: `now` while one is fading (redraw every frame),
    /// else when the next fade starts. `None` when nothing is shown.
    pub fn next_change(&self, now: Instant) -> Option<Instant> {
        self.shown
            .iter()
            .map(|notice| (notice.since + NOTICE_SHOWN).max(now))
            .min()
    }

    /// Drops expired notices; returns the rest, oldest first, with their opacity.
    pub fn visible(&mut self, now: Instant) -> Vec<(&str, f32)> {
        self.shown
            .retain(|notice| now < notice.since + NOTICE_SHOWN + NOTICE_FADE);
        self.shown
            .iter()
            .map(|notice| (notice.text.as_str(), opacity(notice.since, now)))
            .collect()
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
    fn a_keyed_notice_replaces_its_earlier_text() {
        let start = Instant::now();
        let mut notices = Notices::default();
        notices.notify_as("scan", "Scanning… 10", start);
        notices.notify("other", start);
        notices.notify_as("scan", "Scanning… 20", start + Duration::from_secs(1));
        assert_eq!(texts(&notices), ["other", "Scanning… 20"]);
    }

    #[test]
    fn a_dismissed_notice_leaves_at_once_and_alone() {
        let now = Instant::now();
        let mut notices = Notices::default();
        notices.notify_as("loading", "Loading…", now);
        notices.notify("other", now);
        notices.dismiss("loading");
        assert_eq!(texts(&notices), ["other"]);
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

    #[test]
    fn expired_notices_leave_and_the_rest_report_their_fade() {
        let start = Instant::now();
        let mut notices = Notices::default();
        notices.notify("old", start);
        notices.notify("new", start + NOTICE_SHOWN);
        let halfway = start + NOTICE_SHOWN + NOTICE_FADE / 2;
        let visible = notices.visible(halfway);
        assert_eq!(visible.len(), 2);
        assert!((visible[0].1 - 0.5).abs() < 1e-3);
        assert_eq!(visible[1], ("new", 1.0));
        assert_eq!(
            notices.visible(start + NOTICE_SHOWN + NOTICE_FADE),
            [("new", 1.0)]
        );
    }
}
