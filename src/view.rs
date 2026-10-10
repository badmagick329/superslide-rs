//! The view transform: fit, zoom, pan, screen rotation and EXIF orientation. Pure math, no GPU.

#![expect(
    dead_code,
    reason = "scaffold: zoom, pan and rotation arrive with the view issue"
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

/// Where the quad lands on screen. The quad's local coordinates run from (-0.5, -0.5) at the
/// image's top-left to (0.5, 0.5) at its bottom-right; `matrix` (column-major, as WGSL reads it)
/// scales and turns them into window pixels, and `offset` is the window pixel the image centre
/// sits on. Window pixels have y pointing down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub matrix: [[f32; 2]; 2],
    pub offset: [f32; 2],
}

/// The largest scale that shows the whole image, centred, aspect kept.
pub fn fit(image: [u32; 2], window: [u32; 2]) -> Transform {
    let [iw, ih] = image.map(|v| v as f32);
    let [ww, wh] = window.map(|v| v as f32);
    let scale = (ww / iw).min(wh / ih);
    Transform {
        matrix: [[iw * scale, 0.0], [0.0, ih * scale]],
        offset: [ww / 2.0, wh / 2.0],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// On-screen width and height of the fitted quad.
    fn size(t: Transform) -> [f32; 2] {
        [t.matrix[0][0], t.matrix[1][1]]
    }

    #[test]
    fn fit_keeps_aspect_and_fills_the_limiting_axis() {
        // Wide window: height limits.
        assert_eq!(size(fit([400, 200], [1000, 300])), [600.0, 300.0]);
        // Narrow window: width limits.
        assert_eq!(size(fit([400, 200], [100, 900])), [100.0, 50.0]);
    }

    #[test]
    fn fit_centres_the_image() {
        assert_eq!(fit([400, 200], [1000, 300]).offset, [500.0, 150.0]);
    }

    #[test]
    fn fit_holds_for_a_one_pixel_wide_window() {
        let [w, h] = size(fit([384, 256], [1, 800]));
        assert!((w / h - 1.5).abs() < 1e-6);
    }
}
