//! `Preferences`, their defaults and ranges, and the preferences screen's editing state; the
//! `Session` in [`session`].

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};

use crate::info_overlay::domain::InfoOverlay;
use crate::view::domain::MagnificationFilter;

pub mod session;

pub const ADVANCE_LIMIT_MS: RangeInclusive<u32> = 0..=5_000;
/// From 1 s: at 0 the slideshow would advance as fast as images decode.
pub const SLIDESHOW_INTERVAL_S: RangeInclusive<u32> = 1..=3_600;
/// The preload window is kept past the budget, so even the lowest still shows every image.
pub const PRELOAD_BUDGET_MB: RangeInclusive<u32> = 64..=65_536;

/// The owner's persisted options. Defaults follow *Preference defaults* in
/// `docs/context/architecture.md`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
// A file from an older build that lacks a field still loads, with that field at its default.
#[serde(default)]
pub struct Preferences {
    /// Shortest interval between image changes while Space or Backspace is held.
    pub advance_limit_ms: u32,
    pub slideshow_interval_s: u32,
    /// Folder names skipped when opening a folder, matched case-insensitively.
    pub folder_filter: Vec<String>,
    /// GPU texture bytes the preload cache may hold, mips included.
    pub preload_budget_mb: u32,
    pub start_fullscreen: bool,
    pub session_restore: bool,
    pub magnification_filter: MagnificationFilter,
    pub info_overlay: InfoOverlay,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            advance_limit_ms: 500,
            slideshow_interval_s: 10,
            folder_filter: vec!["private".to_owned()],
            preload_budget_mb: 1024,
            start_fullscreen: false,
            session_restore: true,
            magnification_filter: MagnificationFilter::Linear,
            info_overlay: InfoOverlay::Hidden,
        }
    }
}

impl Preferences {
    /// Every number into its range and every folder name trimmed: the file is plain text the
    /// owner may edit by hand.
    pub fn clamped(self) -> Self {
        let clamp =
            |value: u32, range: RangeInclusive<u32>| value.clamp(*range.start(), *range.end());
        Self {
            advance_limit_ms: clamp(self.advance_limit_ms, ADVANCE_LIMIT_MS),
            slideshow_interval_s: clamp(self.slideshow_interval_s, SLIDESHOW_INTERVAL_S),
            preload_budget_mb: clamp(self.preload_budget_mb, PRELOAD_BUDGET_MB),
            folder_filter: folder_filter(&self.folder_filter.join("\n")),
            ..self
        }
    }
}

/// The preferences screen while it is open.
#[derive(Debug)]
pub struct Screen {
    /// The folder filter as typed, one name per line. Kept as text, because reading it back from
    /// `Preferences` every frame would drop the empty line a just-typed Enter makes.
    pub folder_filter: String,
}

impl Screen {
    pub fn new(preferences: &Preferences) -> Self {
        Self {
            folder_filter: preferences.folder_filter.join("\n"),
        }
    }
}

/// One name per line, trimmed; blank lines are dropped.
pub fn folder_filter(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_survive_a_toml_round_trip() {
        let text = toml::to_string(&Preferences::default()).unwrap();
        assert_eq!(
            toml::from_str::<Preferences>(&text).unwrap(),
            Preferences::default()
        );
    }

    #[test]
    fn missing_fields_take_their_defaults() {
        let prefs: Preferences = toml::from_str("advance_limit_ms = 250").unwrap();
        assert_eq!(prefs.advance_limit_ms, 250);
        assert_eq!(
            prefs.slideshow_interval_s,
            Preferences::default().slideshow_interval_s
        );
        assert_eq!(prefs.folder_filter, Preferences::default().folder_filter);
    }

    #[test]
    fn hand_edited_values_are_brought_into_range() {
        let prefs = Preferences {
            advance_limit_ms: 1_000_000,
            slideshow_interval_s: 0,
            preload_budget_mb: 1,
            folder_filter: vec!["  private ".to_owned(), String::new(), "tmp".to_owned()],
            ..Preferences::default()
        }
        .clamped();
        assert_eq!(prefs.advance_limit_ms, 5_000);
        assert_eq!(prefs.slideshow_interval_s, 1);
        assert_eq!(prefs.preload_budget_mb, 64);
        assert_eq!(prefs.folder_filter, ["private", "tmp"]);
        assert_eq!(Preferences::default().clamped(), Preferences::default());
    }

    #[test]
    fn the_folder_filter_is_one_name_per_line() {
        assert_eq!(
            folder_filter("private\n\n  Old Stuff  \r\n"),
            ["private", "Old Stuff"]
        );
        assert!(folder_filter("").is_empty());
        let screen = Screen::new(&Preferences {
            folder_filter: vec!["a".to_owned(), "b".to_owned()],
            ..Preferences::default()
        });
        assert_eq!(screen.folder_filter, "a\nb");
    }
}
