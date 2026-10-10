//! The winit `ApplicationHandler`: turns input into [`Command`]s, owns all state and runs commands.

#![expect(dead_code, reason = "scaffold: the event loop arrives with the window")]

use crate::files::Transfer;
use crate::library::{Direction, SortKey};
use crate::view::{Anchor, PanBy, Turn};

/// Everything the owner can ask for. Every row of `docs/context/keymap.md` maps to a variant;
/// rows that differ only by a parameter (R / L, M / C) share one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Next,
    Previous,
    /// First image of the adjacent group; does nothing unless grouped by subfolder.
    JumpGroup(Direction),
    ToggleSlideshow,
    Pan(PanBy),
    /// Multiplies the zoom by `factor`. Wheel, right drag, + / - and Ctrl+ / Ctrl- differ only in
    /// factor and anchor.
    Zoom {
        factor: f32,
        anchor: Anchor,
    },
    Fit,
    Rotate(Turn),
    ToggleMagnificationFilter,
    CycleInfoOverlay,
    ToggleFullscreen,
    /// Esc: leaves fullscreen, or quits when windowed.
    LeaveFullscreenOrQuit,
    Sort(SortKey),
    /// Gives a new random order even when already shuffled.
    Shuffle,
    ReverseSort,
    ToggleGrouping,
    Open {
        recursive: bool,
    },
    DeleteToRecycleBin,
    Transfer(Transfer),
    CopyToClipboard,
    PasteFromClipboard,
    TogglePreferences,
}
