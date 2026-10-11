//! The notice each file action ends with. Every outcome gets one: Delete has no confirmation, so
//! the notice is how the owner learns what happened.

use std::path::Path;

use crate::file_actions::domain::{Action, Batch, Failure, Transfer};

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

/// A batch's one notice: how many went, and while any failed, the first failure's reason. The
/// failed ones stay marked, so the owner can see which and try again.
pub fn describe_batch(batch: &Batch) -> String {
    let (done, total) = (batch.succeeded, batch.total);
    let count = match batch.failed.is_empty() {
        true => images(total),
        false => format!("{done} of {}", images(total)),
    };
    let head = match &batch.action {
        Action::Delete => format!("{count} moved to the Recycle Bin"),
        Action::Transfer(Transfer::Move, folder) => {
            format!("Moved {count} to {}", folder_name(folder))
        }
        Action::Transfer(Transfer::Copy, folder) => {
            format!("Copied {count} to {}", folder_name(folder))
        }
    };
    let Some((path, failure)) = batch.failed.first() else {
        return head;
    };
    let name = path
        .file_name()
        .unwrap_or(path.as_os_str())
        .to_string_lossy();
    match batch.failed.len() {
        1 => format!("{head}. {name} stays marked: {failure}"),
        failed => format!("{head}. {failed} stay marked; {name}: {failure}"),
    }
}

/// What a batch is about to do, shown until its notice replaces it.
pub fn describe_batch_start(action: &Action, total: usize) -> String {
    let verb = match action {
        Action::Delete => "Deleting",
        Action::Transfer(Transfer::Move, _) => "Moving",
        Action::Transfer(Transfer::Copy, _) => "Copying",
    };
    format!("{verb} {}…", images(total))
}

pub fn images(n: usize) -> String {
    match n {
        1 => "1 image".to_owned(),
        n => format!("{n} images"),
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

    #[test]
    fn a_batch_notice_counts_and_names_the_first_failure() {
        let paths: Vec<PathBuf> = ["a.jpg", "b.jpg", "c.jpg"].map(PathBuf::from).to_vec();
        let mut batch = Batch::new(Action::Transfer(Transfer::Move, r"C:\keep".into()), &paths);
        assert_eq!(describe_batch_start(&batch.action, 3), "Moving 3 images…");
        for path in &paths {
            batch.finish(path, Ok(()));
        }
        assert_eq!(describe_batch(&batch), "Moved 3 images to keep");

        let mut batch = Batch::new(Action::Delete, &paths);
        batch.finish(&paths[0], Ok(()));
        batch.finish(&paths[1], Err(Failure::Failed("in use".to_owned())));
        batch.finish(&paths[2], Ok(()));
        assert_eq!(
            describe_batch(&batch),
            "2 of 3 images moved to the Recycle Bin. b.jpg stays marked: in use"
        );

        let mut batch = Batch::new(
            Action::Transfer(Transfer::Copy, r"C:\keep".into()),
            &paths[..2],
        );
        batch.finish(&paths[1], Err(Failure::Exists));
        batch.finish(&paths[0], Err(Failure::Failed("in use".to_owned())));
        assert_eq!(
            describe_batch(&batch),
            "Copied 0 of 2 images to keep. 2 stay marked; b.jpg: a file of that name is already there"
        );
    }
}
