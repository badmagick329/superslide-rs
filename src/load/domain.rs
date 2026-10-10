//! What decoding produces and the rules it enforces before trusting a file: its content matches
//! its name, its size is within reach, and it fits the GPU. Orientation is applied at draw time,
//! never to the pixels.

use std::fmt;
use std::io;

use crate::image_file::domain::{Format, Orientation};

pub mod preload;

/// Larger images are refused before any pixel buffer exists: a few kilobytes of JPEG can claim
/// 65535 × 65535, which is 17 GB as RGBA. 2^28 pixels is 1 GiB as RGBA, and still takes a
/// 30000 × 8000 panorama.
pub const MAX_PIXELS: u64 = 1 << 28;

pub struct Decoded {
    /// Size of `pixels`: the stored size, or smaller when it was downscaled to fit the GPU.
    pub size: [u32; 2],
    /// Size in the file, before any downscale, for the info overlay.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "scaffold: the info overlay shows it (SSR-17)")
    )]
    pub original_size: [u32; 2],
    /// RGBA8 in sRGB, row-major, no row padding.
    pub pixels: Vec<u8>,
    pub orientation: Orientation,
}

/// Why an image did not decode. `Display` is the one line the error card shows.
#[derive(Debug)]
pub enum DecodeError {
    /// The file could not be read.
    Io(io::Error),
    /// Not something this viewer opens: misnamed or too large.
    Unsupported(String),
    /// The file is the format its name says, but its data is broken or cut short.
    Corrupt(String),
}

impl DecodeError {
    /// Decoder messages can run to several lines; the error card has room for one.
    pub fn damaged(message: impl fmt::Display) -> Self {
        let message = message.to_string();
        let first = message.lines().next().unwrap_or_default().trim();
        DecodeError::Corrupt(format!("Damaged image: {first}"))
    }

    pub fn cut_short() -> Self {
        DecodeError::Corrupt("Damaged image: the file is cut short".to_owned())
    }
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Io(e) => write!(f, "Cannot read the file: {e}"),
            DecodeError::Unsupported(reason) | DecodeError::Corrupt(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for DecodeError {}

/// A file whose content is another format gets told so, instead of a decoder's confusing error.
pub fn confirm(named: Format, data: &[u8]) -> Result<(), DecodeError> {
    if data.is_empty() {
        return Err(DecodeError::Corrupt("The file is empty".to_owned()));
    }
    match Format::sniff(data) {
        Some(actual) if actual == named => Ok(()),
        Some(actual) => Err(DecodeError::Unsupported(format!(
            "Misnamed: a {} file, not {}",
            actual.name(),
            named.name()
        ))),
        // Without a signature to check, a TGA is left to its decoder to judge.
        None if named == Format::Tga => Ok(()),
        None => Err(DecodeError::Corrupt(format!("Not a {} file", named.name()))),
    }
}

/// Called with the header's size, before any pixel buffer is allocated.
pub fn check_size(width: u32, height: u32) -> Result<(), DecodeError> {
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(DecodeError::Unsupported(format!(
            "Too large to open: {width} × {height} pixels"
        )));
    }
    Ok(())
}

/// The largest size within `max_dim` on both sides with the same aspect; the long side lands on
/// `max_dim` exactly.
pub fn fit_within(size: [u32; 2], max_dim: u32) -> [u32; 2] {
    let long = size[0].max(size[1]);
    if long <= max_dim {
        return size;
    }
    size.map(|side| {
        let scaled = (u64::from(side) * u64::from(max_dim) + u64::from(long) / 2) / u64::from(long);
        scaled.max(1) as u32
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitting_keeps_aspect_and_lands_the_long_side_on_the_limit() {
        assert_eq!(fit_within([300, 100], 64), [64, 21]);
        assert_eq!(fit_within([100, 300], 64), [21, 64]);
        assert_eq!(fit_within([64, 10], 64), [64, 10]);
        // The short side never rounds to nothing.
        assert_eq!(fit_within([10_000, 1], 100), [100, 1]);
    }

    #[test]
    fn damage_reports_keep_one_line() {
        let error = DecodeError::damaged("bad marker\nat offset 12");
        assert_eq!(error.to_string(), "Damaged image: bad marker");
    }
}
