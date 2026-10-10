//! What persists between runs, beside the executable: the owner's preferences
//! (`superslide-prefs.toml`) and the screen P opens to edit them, and the session
//! (`superslide-session.toml`) that session restore reopens.

pub mod domain;
pub mod infrastructure;
pub mod presentation;
