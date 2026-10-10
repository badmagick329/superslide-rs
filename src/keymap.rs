//! Maps keys and mouse gestures to [`Command`]. The only place bindings are decided; the table is
//! `docs/context/keymap.md`.

use winit::keyboard::{Key, NamedKey};

use crate::app::Command;

/// The command a key press runs, if any. `repeat` is true for auto-repeat while the key is held.
pub fn for_key(key: &Key, repeat: bool) -> Option<Command> {
    // Held toggles would flicker between states, and a held Esc would leave fullscreen then quit.
    if repeat {
        return None;
    }
    match key {
        Key::Named(NamedKey::F11 | NamedKey::Enter) => Some(Command::ToggleFullscreen),
        Key::Named(NamedKey::Escape) => Some(Command::LeaveFullscreenOrQuit),
        _ => None,
    }
}
