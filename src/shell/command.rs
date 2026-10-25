//! Everything the owner can ask for, as data: `keymap` produces commands and `app` runs them.

use crate::browse::domain::{Direction, SortKey, Travel};
use crate::file_actions::domain::Transfer;
use crate::view::domain::{Anchor, PanBy, Turn};

/// Everything the owner can ask for. Every row of `docs/context/keymap.md`'s tables maps to a
/// variant; rows that differ only by a parameter (R / L, M / C) share one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    /// Moves through the image list. `repeat` is key repeat from a held key, which the advance
    /// limit paces; a press or click always moves.
    Advance {
        travel: Travel,
        repeat: bool,
    },
    /// First image of the adjacent group; does nothing unless grouped by subfolder.
    JumpGroup(Direction),
    /// Vim's `gg` / `G`: [`Command::JumpGroup`] when there are groups to jump between, else the
    /// first or last image.
    JumpGroupOrEnd(Direction),
    /// Vim's `[count]G`: the image at this 1-based position, or the last past the end.
    GoTo(usize),
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
    /// Classic Esc: leaves fullscreen, or quits when windowed.
    LeaveFullscreenOrQuit,
    /// Vim's Esc, which Vim users press out of habit, so it never quits.
    LeaveFullscreen,
    Quit,
    Sort(SortKey),
    /// Gives a new random order even when already shuffled.
    Shuffle,
    ReverseSort,
    ToggleGrouping,
    /// Vim's `[count]m`: marks or unmarks this many images from the current one on.
    ToggleMarks(usize),
    ClearMarks,
    /// Vim's `n` / `N`: the next or previous marked image, wrapping.
    NextMarked(Direction),
    Open {
        recursive: bool,
    },
    /// This and the next three act on every marked image while any is marked.
    DeleteToRecycleBin,
    Transfer(Transfer),
    CopyToClipboard,
    /// The image's full path as text, where `CopyToClipboard` copies its pixels.
    CopyPathToClipboard,
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
