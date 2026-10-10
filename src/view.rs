//! The view transform: fit, zoom, pan, screen rotation and EXIF orientation. Pure math, no GPU.

use crate::decode::Orientation;

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
/// stored image's top-left to (0.5, 0.5) at its bottom-right; `matrix` (column-major, as WGSL
/// reads it) scales and turns them into window pixels, and `offset` is the window pixel the
/// image centre sits on. Window pixels have y pointing down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub matrix: [[f32; 2]; 2],
    pub offset: [f32; 2],
}

/// Zoom-out limit, relative to fit.
const MIN_ZOOM: f32 = 1.0 / 8.0;
/// Zoom-in limit, in screen pixels per image pixel. Fit wins when it is already larger, so a
/// tiny image can always be shown at fit.
const MAX_SCALE: f32 = 32.0;

/// How one image is shown. State is kept relative to the image, so a window resize keeps the
/// same image point centred at the same zoom relative to fit.
#[derive(Clone, Debug)]
pub struct View {
    /// Stored pixel size, before orientation.
    image: [f32; 2],
    window: [f32; 2],
    orientation: Orientation,
    /// Clockwise quarter-turns of screen rotation, 0 to 3.
    turns: u8,
    /// 1 is fit.
    zoom: f32,
    /// The image point at the window centre, in quad-local coordinates. Kept in image space, not
    /// screen space, so resizing leaves the same detail in view.
    centre: [f32; 2],
}

impl View {
    /// Opens at fit and centred. `turns` carries the screen rotation an image kept from an
    /// earlier visit.
    pub fn new(image: [u32; 2], orientation: Orientation, turns: u8, window: [u32; 2]) -> Self {
        Self {
            image: image.map(|v| v as f32),
            window: window.map(|v| v as f32),
            orientation,
            turns: turns % 4,
            zoom: 1.0,
            centre: [0.0, 0.0],
        }
    }

    /// Read when leaving an image, so its screen rotation survives navigating away and back.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "scaffold: read once navigation exists")
    )]
    pub fn turns(&self) -> u8 {
        self.turns
    }

    pub fn set_window(&mut self, window: [u32; 2]) {
        self.window = window.map(|v| v as f32);
        self.clamp();
    }

    pub fn fit(&mut self) {
        self.zoom = 1.0;
        self.centre = [0.0, 0.0];
    }

    /// Turning changes which way the image is long, so the old zoom and pan would land somewhere
    /// arbitrary; the image returns to fit instead.
    pub fn rotate(&mut self, turn: Turn) {
        self.turns = match turn {
            Turn::Right => (self.turns + 1) % 4,
            Turn::Left => (self.turns + 3) % 4,
        };
        self.fit();
    }

    /// Multiplies the zoom by `factor`, within the limits, keeping the image point under
    /// `anchor` where it is unless the pan clamp has to move it.
    pub fn zoom(&mut self, factor: f32, anchor: Anchor) {
        let max_zoom = (MAX_SCALE / self.fit_scale()).max(1.0);
        let zoom = (self.zoom * factor).clamp(MIN_ZOOM, max_zoom);
        let anchor = match anchor {
            Anchor::Centre => self.window_centre(),
            Anchor::Cursor { x, y } => [x, y],
        };
        let fixed = self.image_point_at(anchor);
        // The window centre's distance from the fixed point, in image space, shrinks by the
        // same ratio the image grows.
        let k = zoom / self.zoom;
        self.centre = [0, 1].map(|i| fixed[i] + (self.centre[i] - fixed[i]) / k);
        self.zoom = zoom;
        self.clamp();
    }

    pub fn pan(&mut self, by: PanBy) {
        let [dx, dy] = match by {
            PanBy::Pixels { dx, dy } => [dx, dy],
            PanBy::WindowFraction { dx, dy } => [dx * self.window[0], dy * self.window[1]],
        };
        let [x, y] = self.image_centre_on_screen();
        self.put_image_centre(self.clamped([x + dx, y + dy]));
    }

    pub fn transform(&self) -> Transform {
        Transform {
            matrix: self.linear().columns(),
            offset: self.image_centre_on_screen(),
        }
    }

    /// Orientation, then screen rotation: the unit map from stored to on-screen directions.
    fn unit(&self) -> Mat2 {
        let mut m = orientation_matrix(self.orientation);
        for _ in 0..self.turns {
            m = Mat2::CLOCKWISE.mul(m);
        }
        m
    }

    /// Image size as it stands on screen, before scaling: width and height swap when the unit
    /// map sends x to vertical.
    fn upright(&self) -> [f32; 2] {
        let [w, h] = self.image;
        if self.unit().0[0][0] == 0.0 {
            [h, w]
        } else {
            [w, h]
        }
    }

    fn fit_scale(&self) -> f32 {
        let [uw, uh] = self.upright();
        (self.window[0] / uw).min(self.window[1] / uh)
    }

    /// Screen pixels per image pixel.
    fn scale(&self) -> f32 {
        self.zoom * self.fit_scale()
    }

    /// Quad-local coordinates to screen pixels, relative to the image centre.
    fn linear(&self) -> Mat2 {
        self.unit()
            .scaled(self.scale())
            .mul(Mat2([[self.image[0], 0.0], [0.0, self.image[1]]]))
    }

    /// The inverse of `linear`. The unit map is a signed permutation, so its transpose inverts it.
    fn inverse_linear(&self) -> Mat2 {
        Mat2([[1.0 / self.image[0], 0.0], [0.0, 1.0 / self.image[1]]])
            .mul(self.unit().transpose())
            .scaled(1.0 / self.scale())
    }

    fn window_centre(&self) -> [f32; 2] {
        self.window.map(|v| v / 2.0)
    }

    fn image_centre_on_screen(&self) -> [f32; 2] {
        let c = self.window_centre();
        let m = self.linear().apply(self.centre);
        [c[0] - m[0], c[1] - m[1]]
    }

    fn put_image_centre(&mut self, at: [f32; 2]) {
        let c = self.window_centre();
        self.centre = self.inverse_linear().apply([c[0] - at[0], c[1] - at[1]]);
    }

    fn image_point_at(&self, screen: [f32; 2]) -> [f32; 2] {
        let e = self.image_centre_on_screen();
        self.inverse_linear()
            .apply([screen[0] - e[0], screen[1] - e[1]])
    }

    /// Per axis: an image narrower than the window is centred on it; a wider one must cover it,
    /// so no edge comes inside the window.
    fn clamped(&self, at: [f32; 2]) -> [f32; 2] {
        let upright = self.upright();
        let scale = self.scale();
        [0, 1].map(|i| {
            let size = upright[i] * scale;
            let window = self.window[i];
            if size <= window {
                window / 2.0
            } else {
                at[i].clamp(window - size / 2.0, size / 2.0)
            }
        })
    }

    fn clamp(&mut self) {
        let at = self.clamped(self.image_centre_on_screen());
        self.put_image_centre(at);
    }
}

/// How EXIF orientation turns stored pixels upright, in y-down coordinates.
fn orientation_matrix(orientation: Orientation) -> Mat2 {
    Mat2(match orientation {
        Orientation::Normal => [[1.0, 0.0], [0.0, 1.0]],
        Orientation::MirrorHorizontal => [[-1.0, 0.0], [0.0, 1.0]],
        Orientation::Rotate180 => [[-1.0, 0.0], [0.0, -1.0]],
        Orientation::MirrorVertical => [[1.0, 0.0], [0.0, -1.0]],
        Orientation::Transpose => [[0.0, 1.0], [1.0, 0.0]],
        Orientation::Rotate90 => [[0.0, -1.0], [1.0, 0.0]],
        Orientation::Transverse => [[0.0, -1.0], [-1.0, 0.0]],
        Orientation::Rotate270 => [[0.0, 1.0], [-1.0, 0.0]],
    })
}

/// A 2×2 matrix stored by rows, which reads naturally here; `columns` converts for the GPU.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Mat2([[f32; 2]; 2]);

impl Mat2 {
    /// A quarter-turn clockwise on screen, where y points down: right goes to down.
    const CLOCKWISE: Self = Self([[0.0, -1.0], [1.0, 0.0]]);

    fn mul(self, other: Self) -> Self {
        let (a, b) = (self.0, other.0);
        Self([0, 1].map(|r| [0, 1].map(|c| a[r][0] * b[0][c] + a[r][1] * b[1][c])))
    }

    fn apply(self, v: [f32; 2]) -> [f32; 2] {
        let m = self.0;
        [
            m[0][0] * v[0] + m[0][1] * v[1],
            m[1][0] * v[0] + m[1][1] * v[1],
        ]
    }

    fn scaled(self, s: f32) -> Self {
        Self(self.0.map(|row| row.map(|v| v * s)))
    }

    fn transpose(self) -> Self {
        let m = self.0;
        Self([[m[0][0], m[1][0]], [m[0][1], m[1][1]]])
    }

    fn columns(self) -> [[f32; 2]; 2] {
        self.transpose().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f32 = 1e-3;

    fn close(a: [f32; 2], b: [f32; 2]) -> bool {
        (a[0] - b[0]).abs() < EPS && (a[1] - b[1]).abs() < EPS
    }

    /// Where a quad-local point lands on screen, computed the way the shader does.
    fn on_screen(t: Transform, p: [f32; 2]) -> [f32; 2] {
        let [c0, c1] = t.matrix;
        [
            c0[0] * p[0] + c1[0] * p[1] + t.offset[0],
            c0[1] * p[0] + c1[1] * p[1] + t.offset[1],
        ]
    }

    /// The on-screen bounding box of the image: [min x, min y, max x, max y].
    fn rect(view: &View) -> [f32; 4] {
        let t = view.transform();
        let corners = [[-0.5, -0.5], [0.5, -0.5], [-0.5, 0.5], [0.5, 0.5]].map(|p| on_screen(t, p));
        let xs = corners.map(|c| c[0]);
        let ys = corners.map(|c| c[1]);
        let min = |v: [f32; 4]| v.into_iter().fold(f32::INFINITY, f32::min);
        let max = |v: [f32; 4]| v.into_iter().fold(f32::NEG_INFINITY, f32::max);
        [min(xs), min(ys), max(xs), max(ys)]
    }

    fn size(view: &View) -> [f32; 2] {
        let r = rect(view);
        [r[2] - r[0], r[3] - r[1]]
    }

    /// The quad-local point under a screen pixel, by inverting the transform.
    fn under(view: &View, p: [f32; 2]) -> [f32; 2] {
        let t = view.transform();
        let [[a, c], [b, d]] = t.matrix;
        let det = a * d - b * c;
        let (x, y) = (p[0] - t.offset[0], p[1] - t.offset[1]);
        [(d * x - b * y) / det, (a * y - c * x) / det]
    }

    #[test]
    fn each_orientation_puts_the_stored_corners_where_exif_says() {
        use Orientation::*;
        // Where the stored top-left and top-right corners must land, with the window exactly the
        // upright image so fit fills it.
        let cases = [
            (Normal, "top-left", "top-right"),
            (MirrorHorizontal, "top-right", "top-left"),
            (Rotate180, "bottom-right", "bottom-left"),
            (MirrorVertical, "bottom-left", "bottom-right"),
            (Transpose, "top-left", "bottom-left"),
            (Rotate90, "top-right", "bottom-right"),
            (Transverse, "bottom-right", "top-right"),
            (Rotate270, "bottom-left", "top-left"),
        ];
        for (orientation, want_tl, want_tr) in cases {
            let swapped = matches!(orientation, Transpose | Rotate90 | Transverse | Rotate270);
            let window = if swapped { [200, 300] } else { [300, 200] };
            let [w, h] = window.map(|v| v as f32);
            let corner = |name| match name {
                "top-left" => [0.0, 0.0],
                "top-right" => [w, 0.0],
                "bottom-left" => [0.0, h],
                _ => [w, h],
            };
            let t = View::new([300, 200], orientation, 0, window).transform();
            let tl = on_screen(t, [-0.5, -0.5]);
            let tr = on_screen(t, [0.5, -0.5]);
            assert!(
                close(tl, corner(want_tl)),
                "{orientation:?} top-left at {tl:?}"
            );
            assert!(
                close(tr, corner(want_tr)),
                "{orientation:?} top-right at {tr:?}"
            );
        }
    }

    #[test]
    fn screen_rotation_composes_with_orientation() {
        // A right turn moves the stored top-left to the screen's top-right.
        let mut view = View::new([300, 200], Orientation::Normal, 0, [200, 300]);
        view.rotate(Turn::Right);
        assert!(close(
            on_screen(view.transform(), [-0.5, -0.5]),
            [200.0, 0.0]
        ));
        // A turn left undoes a photo that needs a turn right.
        let mut view = View::new([300, 200], Orientation::Rotate90, 0, [300, 200]);
        view.rotate(Turn::Left);
        let normal = View::new([300, 200], Orientation::Normal, 0, [300, 200]);
        assert_eq!(view.transform(), normal.transform());
        assert_eq!(view.turns(), 3);
    }

    #[test]
    fn fit_is_correct_for_every_shape_before_and_after_rotation() {
        let landscape = [400, 200];
        let portrait = [200, 400];
        let wide = [1000, 500];
        let tall = [500, 1000];
        // (image, window, turns, expected on-screen size)
        let cases = [
            (landscape, wide, 0, [1000.0, 500.0]),
            (landscape, tall, 0, [500.0, 250.0]),
            (portrait, wide, 0, [250.0, 500.0]),
            (portrait, tall, 0, [500.0, 1000.0]),
            (landscape, wide, 1, [250.0, 500.0]),
            (landscape, tall, 1, [500.0, 1000.0]),
            (portrait, wide, 1, [1000.0, 500.0]),
            (portrait, tall, 1, [500.0, 250.0]),
        ];
        for (image, window, turns, want) in cases {
            let view = View::new(image, Orientation::Normal, turns, window);
            assert!(
                close(size(&view), want),
                "{image:?} in {window:?}, {turns} turns"
            );
            let r = rect(&view);
            let centre = [(r[0] + r[2]) / 2.0, (r[1] + r[3]) / 2.0];
            assert!(close(centre, window.map(|v| v as f32 / 2.0)));
        }
    }

    #[test]
    fn fit_holds_for_a_one_pixel_wide_window() {
        let [w, h] = size(&View::new([384, 256], Orientation::Normal, 0, [1, 800]));
        assert!((w / h - 1.5).abs() < 1e-4);
    }

    #[test]
    fn anchored_zoom_keeps_the_point_under_the_cursor() {
        // Upright, the image has the window's aspect, so once zoomed in it covers the window and
        // the clamp stays out of the way.
        let mut view = View::new([400, 300], Orientation::Rotate90, 0, [600, 800]);
        for (factor, cursor) in [
            (2.0, [100.0, 150.0]),
            (1.5, [500.0, 700.0]),
            (0.8, [300.0, 50.0]),
        ] {
            let before = under(&view, cursor);
            view.zoom(
                factor,
                Anchor::Cursor {
                    x: cursor[0],
                    y: cursor[1],
                },
            );
            assert!(
                close(under(&view, cursor), before),
                "zoom {factor} at {cursor:?}"
            );
        }
    }

    #[test]
    fn pan_clamp_holds_at_every_zoom() {
        let window = [800.0, 600.0];
        for zoom in [0.125, 0.5, 1.0, 1.3, 2.0, 7.0] {
            for (dx, dy) in [
                (1e5, 0.0),
                (-1e5, 0.0),
                (0.0, 1e5),
                (0.0, -1e5),
                (1e5, -1e5),
            ] {
                let mut view = View::new([1000, 500], Orientation::Normal, 0, [800, 600]);
                view.zoom(zoom, Anchor::Centre);
                view.pan(PanBy::Pixels { dx, dy });
                let r = rect(&view);
                for axis in 0..2 {
                    let (lo, hi) = (r[axis], r[axis + 2]);
                    if hi - lo <= window[axis] + EPS {
                        assert!(
                            (lo + hi - window[axis]).abs() < EPS,
                            "zoom {zoom}: not centred"
                        );
                    } else {
                        assert!(
                            lo <= EPS && hi >= window[axis] - EPS,
                            "zoom {zoom}: edge inside"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn zoom_stops_at_its_limits_but_fit_stays_reachable() {
        let mut view = View::new([1000, 500], Orientation::Normal, 0, [1000, 500]);
        view.zoom(1e6, Anchor::Centre);
        assert!((size(&view)[0] - 1000.0 * MAX_SCALE).abs() < 1.0);
        view.zoom(1e-6, Anchor::Centre);
        assert!((size(&view)[0] - 1000.0 * MIN_ZOOM).abs() < EPS);

        // Fit is already past 32× for a 16 px icon in a large window: zooming in does nothing,
        // and zooming out then Fit comes back to it.
        let mut view = View::new([16, 16], Orientation::Normal, 0, [1600, 1600]);
        view.zoom(2.0, Anchor::Centre);
        assert!(close(size(&view), [1600.0, 1600.0]));
        view.zoom(0.5, Anchor::Centre);
        view.fit();
        assert!(close(size(&view), [1600.0, 1600.0]));
    }

    #[test]
    fn resizing_keeps_the_centred_detail() {
        let mut view = View::new([1000, 1000], Orientation::Normal, 0, [500, 500]);
        view.zoom(4.0, Anchor::Cursor { x: 100.0, y: 100.0 });
        let before = under(&view, [250.0, 250.0]);
        view.set_window([800, 800]);
        assert!(close(under(&view, [400.0, 400.0]), before));
    }
}
