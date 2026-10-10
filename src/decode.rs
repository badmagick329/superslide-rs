//! Turns a path into RGBA pixels plus their EXIF orientation: format dispatch, orientation read,
//! oversize downscale. Orientation is applied at draw time, never to the pixels here.

#![expect(dead_code, reason = "scaffold: decoding arrives with the decode issue")]

/// EXIF orientation tag values 1 to 8: how the stored pixels must be turned to stand upright.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Orientation {
    #[default]
    Normal,
    MirrorHorizontal,
    Rotate180,
    MirrorVertical,
    /// Mirror horizontally, then rotate 270° clockwise.
    Transpose,
    Rotate90,
    /// Mirror horizontally, then rotate 90° clockwise.
    Transverse,
    Rotate270,
}

pub struct Decoded {
    pub width: u32,
    pub height: u32,
    /// RGBA8 in sRGB, row-major, no row padding.
    pub pixels: Vec<u8>,
    pub orientation: Orientation,
}
