//! Ctrl+C and Ctrl+V outcomes, the check a pasted image passes before it is trusted, and the
//! text Ctrl+Shift+C puts on the clipboard.

use std::fmt;
use std::path::PathBuf;

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

/// One path per line, as Windows text separates lines. No quotes, so a path pastes cleanly into
/// a chat, a text field or an Open dialog, and no trailing newline, so pasting one path into a
/// terminal does not run it.
pub fn path_list(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\r\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_list_is_one_unquoted_path_per_line_with_no_trailing_newline() {
        let list = |paths: &[&str]| path_list(&paths.iter().map(PathBuf::from).collect::<Vec<_>>());
        assert_eq!(list(&[r"C:\photos\a.jpg"]), r"C:\photos\a.jpg");
        assert_eq!(
            list(&[r"C:\photos\a.jpg", r"C:\my photos\b c.png", r"D:\d.bmp"]),
            "C:\\photos\\a.jpg\r\nC:\\my photos\\b c.png\r\nD:\\d.bmp"
        );
    }

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
