//! `Preferences` and their defaults. `Session` and the TOML file arrive with SSR-18 and SSR-19.

#![expect(
    dead_code,
    reason = "scaffold: fields with no reader yet are read once SSR-18 loads the file"
)]

use serde::{Deserialize, Serialize};

use crate::info_overlay::domain::InfoOverlay;
use crate::view::domain::MagnificationFilter;

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
}
