//! The notices Ctrl+C, Ctrl+Shift+C and Ctrl+V end with.

use crate::clipboard::domain::{CopyFailure, PasteFailure};

pub fn copied(file_name: &str, result: &Result<(), CopyFailure>) -> String {
    match result {
        Ok(()) => format!("Copied {file_name} to the clipboard"),
        Err(failure) => format!("Cannot copy {file_name}: {failure}"),
    }
}

/// `images` is already worded, such as `3 images`.
pub fn copied_files(images: &str, result: &Result<(), CopyFailure>) -> String {
    match result {
        Ok(()) => format!("Copied {images} to the clipboard as files"),
        Err(failure) => format!("Cannot copy {images} to the clipboard: {failure}"),
    }
}

/// Ctrl+Shift+C's notice, naming the one image whose path went or counting several.
pub fn copied_paths(paths: &Paths, result: &Result<(), CopyFailure>) -> String {
    let subject = match paths {
        Paths::One { file_name } => format!("the path of {file_name}"),
        Paths::Several { images } => format!("the paths of {images}"),
    };
    match result {
        Ok(()) => format!("Copied {subject}"),
        Err(failure) => format!("Cannot copy {subject}: {failure}"),
    }
}

/// Whose paths a Ctrl+Shift+C copied. A single marked image is `One`, which reads better than
/// "the paths of 1 image".
pub enum Paths<'a> {
    One {
        file_name: &'a str,
    },
    /// `images` is already worded, such as `3 images`.
    Several {
        images: &'a str,
    },
}

pub fn paste_failed(failure: &PasteFailure) -> String {
    match failure {
        PasteFailure::NoImage => "No image on the clipboard".to_owned(),
        PasteFailure::Busy => "Another app is using the clipboard; try again".to_owned(),
        PasteFailure::TooLarge => "The clipboard image is too large to open".to_owned(),
        PasteFailure::Failed(reason) => format!("Cannot read the clipboard: {reason}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_copy_names_one_image_and_counts_several() {
        let one = Paths::One { file_name: "a.jpg" };
        let three = Paths::Several { images: "3 images" };
        let busy = Err(CopyFailure::Clipboard("busy".to_owned()));
        assert_eq!(copied_paths(&one, &Ok(())), "Copied the path of a.jpg");
        assert_eq!(
            copied_paths(&three, &Ok(())),
            "Copied the paths of 3 images"
        );
        assert_eq!(
            copied_paths(&one, &busy),
            "Cannot copy the path of a.jpg: busy"
        );
        assert_eq!(
            copied_paths(&three, &busy),
            "Cannot copy the paths of 3 images: busy"
        );
    }
}
