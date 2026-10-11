//! The help overlay: every binding, listed inside the app, opened with ?. The rows come from
//! the active keymap's `BINDINGS` in `shell::keymap`, the same table the keymap is tested
//! against, so the list cannot drift from the keys.

pub mod domain;
pub mod presentation;
