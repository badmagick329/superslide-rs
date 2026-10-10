//! The notices Ctrl+C and Ctrl+V end with.

use crate::clipboard::domain::{CopyFailure, PasteFailure};

pub fn copied(file_name: &str, result: &Result<(), CopyFailure>) -> String {
    match result {
        Ok(()) => format!("Copied {file_name} to the clipboard"),
        Err(failure) => format!("Cannot copy {file_name}: {failure}"),
    }
}

pub fn paste_failed(failure: &PasteFailure) -> String {
    match failure {
        PasteFailure::NoImage => "No image on the clipboard".to_owned(),
        PasteFailure::Busy => "Another app is using the clipboard; try again".to_owned(),
        PasteFailure::TooLarge => "The clipboard image is too large to open".to_owned(),
        PasteFailure::Failed(reason) => format!("Cannot read the clipboard: {reason}"),
    }
}
