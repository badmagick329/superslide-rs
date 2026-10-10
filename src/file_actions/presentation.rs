//! The notice each file action ends with. Every outcome gets one: Delete has no confirmation, so
//! the notice is how the owner learns what happened.

use std::path::Path;

use crate::file_actions::domain::{Action, Failure, Transfer};

pub fn describe(file_name: &str, action: &Action, result: &Result<(), Failure>) -> String {
    match (action, result) {
        (Action::Delete, Ok(())) => format!("{file_name} moved to the Recycle Bin"),
        (Action::Delete, Err(failure)) => format!("Cannot delete {file_name}: {failure}"),
        (Action::Transfer(transfer, folder), result) => {
            let folder = folder_name(folder);
            match (transfer, result) {
                (Transfer::Move, Ok(())) => format!("Moved {file_name} to {folder}"),
                (Transfer::Copy, Ok(())) => format!("Copied {file_name} to {folder}"),
                (Transfer::Move, Err(failure)) => {
                    format!("Cannot move {file_name} to {folder}: {failure}")
                }
                (Transfer::Copy, Err(failure)) => {
                    format!("Cannot copy {file_name} to {folder}: {failure}")
                }
            }
        }
    }
}

/// The folder's own name: a full path wraps the notice over several lines, and the owner just
/// picked the folder. A drive root has no name, so it shows whole.
fn folder_name(folder: &Path) -> String {
    folder
        .file_name()
        .unwrap_or(folder.as_os_str())
        .to_string_lossy()
        .into_owned()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn notices_name_the_folder_not_its_path() {
        let to = |folder: &str| Action::Transfer(Transfer::Move, PathBuf::from(folder));
        assert_eq!(
            describe("a.jpg", &to(r"C:\photos\keep"), &Ok(())),
            "Moved a.jpg to keep"
        );
        assert_eq!(
            describe("a.jpg", &to(r"D:\"), &Ok(())),
            r"Moved a.jpg to D:\"
        );
        assert_eq!(
            describe("a.jpg", &to(r"C:\keep"), &Err(Failure::Exists)),
            "Cannot move a.jpg to keep: a file of that name is already there"
        );
    }
}
