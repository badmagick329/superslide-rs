//! The help overlay: every binding, listed inside the app, opened with ?. The rows come from
//! `shell::keymap::BINDINGS`, the same table the keymap is tested against, so the list cannot
//! drift from the keys.

pub mod domain;
pub mod presentation;
