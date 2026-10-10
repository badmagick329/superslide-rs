//! The view transform: fit, zoom, pan, screen rotation and EXIF orientation. Pure math, no GPU.

#![expect(
    dead_code,
    reason = "scaffold: the transform arrives with the view issue"
)]

/// One quarter-turn of screen rotation; R turns right (clockwise), L left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Turn {
    Right,
    Left,
}

/// The point a zoom keeps fixed on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Anchor {
    /// Keys zoom about the window centre.
    Centre,
    /// Wheel and right drag zoom about the cursor, in window pixels.
    Cursor { x: f32, y: f32 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PanBy {
    /// Left drag: the image follows the cursor.
    Pixels { dx: f32, dy: f32 },
    /// Arrow keys move a fraction of the window, so pan speed scales with window size.
    WindowFraction { dx: f32, dy: f32 },
}
