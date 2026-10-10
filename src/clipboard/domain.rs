//! Ctrl+C and Ctrl+V outcomes, and the check a pasted image passes before it is trusted.

use std::fmt;

use crate::load::domain::{DecodeError, MAX_PIXELS};

#[derive(Debug, PartialEq, Eq)]
pub enum CopyFailure {
    /// The file would not decode; the error card's one line.
    Decode(String),
    Clipboard(String),
}

impl fmt::Display for CopyFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CopyFailure::Decode(reason) | CopyFailure::Clipboard(reason) => f.write_str(reason),
        }
    }
}

impl From<DecodeError> for CopyFailure {
    fn from(error: DecodeError) -> Self {
        CopyFailure::Decode(error.to_string())
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum PasteFailure {
    /// Empty, or holding text or files rather than an image.
    NoImage,
    /// Another app has the clipboard open.
    Busy,
    TooLarge,
    Failed(String),
}

/// The clipboard is written by any app, so its image is checked like a file: the pixel count is
/// within `MAX_PIXELS` and the buffer holds exactly `width × height` RGBA pixels. Returns the
/// size as the GPU path takes it.
pub fn check_pasted(width: usize, height: usize, bytes: usize) -> Result<[u32; 2], PasteFailure> {
    let (Ok(w), Ok(h)) = (u32::try_from(width), u32::try_from(height)) else {
        return Err(PasteFailure::TooLarge);
    };
    if w == 0 || h == 0 {
        return Err(PasteFailure::NoImage);
    }
    if u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err(PasteFailure::TooLarge);
    }
    if bytes as u64 != u64::from(w) * u64::from(h) * 4 {
        return Err(PasteFailure::Failed(
            "the image data does not match its size".to_owned(),
        ));
    }
    Ok([w, h])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_images_are_checked_like_files() {
        assert_eq!(check_pasted(3, 2, 24), Ok([3, 2]));
        assert_eq!(check_pasted(0, 2, 0), Err(PasteFailure::NoImage));
        assert_eq!(
            check_pasted(1 << 15, 1 << 15, 0),
            Err(PasteFailure::TooLarge)
        );
        assert!(matches!(
            check_pasted(3, 2, 23),
            Err(PasteFailure::Failed(_))
        ));
    }
}
