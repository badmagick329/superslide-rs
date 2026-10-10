//! Everything the owner can ask for, as data: `keymap` produces commands and `app` runs them.

use crate::browse::domain::{Direction, SortKey};
use crate::file_actions::domain::Transfer;
use crate::view::domain::{Anchor, PanBy, Turn};

/// Everything the owner can ask for. Every row of `docs/context/keymap.md` maps to a variant;
/// rows that differ only by a parameter (R / L, M / C) share one.
#[derive(Clone, Copy, Debug, PartialEq)]
#[expect(dead_code, reason = "scaffold: most commands arrive with later issues")]
pub enum Command {
    /// Next or previous image. `repeat` is key repeat from a held Space or Backspace, which the
    /// advance limit paces; a press or click always advances.
    Advance {
        direction: Direction,
        repeat: bool,
    },
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
    /// ? opens the help overlay; while it is open, ? closes it.
    ToggleHelp,
    /// / focuses the help overlay's filter. Does nothing while the overlay is closed.
    FocusHelpFilter,
    /// F9 in debug builds: a fixed notice, for checking notice timing and stacking by hand.
    #[cfg(debug_assertions)]
    TestNotice,
}
