//! What delete, move and copy ask for, how each can end, and a batch of them over the marked
//! images. No filesystem access: which drives have a Recycle Bin is the infrastructure's call.

use std::collections::HashSet;
use std::fmt;
use std::path::{Path, PathBuf};

/// Moves or copies an image to a folder. Neither overwrites an existing file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    Move,
    Copy,
}

/// Where a deleted image goes, decided by its drive. Neither asks first.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Deletion {
    /// The Recycle Bin is the undo.
    ToRecycleBin,
    /// A network drive has no Recycle Bin, and deleting there is expected to be for good.
    ForGood,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    Delete(Deletion),
    /// Into this folder, under the same file name.
    Transfer(Transfer, PathBuf),
}

impl Action {
    /// Whether success means the file is gone from where it was, so it leaves the image list.
    pub fn removes(&self) -> bool {
        matches!(
            self,
            Action::Delete(_) | Action::Transfer(Transfer::Move, _)
        )
    }
}

/// Why an action did not happen, or happened only in part. `Display` is the notice's reason.
#[derive(Debug, PartialEq, Eq)]
pub enum Failure {
    /// The target folder already holds the file name; neither file was touched.
    Exists,
    /// Nothing changed: the source is where it was and no target file is left behind.
    Failed(String),
    /// A move across drives copied the file but could not remove the original, so both exist.
    OriginalKept(String),
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Failure::Exists => f.write_str("a file of that name is already there"),
            Failure::Failed(reason) => f.write_str(reason),
            Failure::OriginalKept(reason) => write!(f, "copied, but the original stays: {reason}"),
        }
    }
}

/// One delete, move or copy of every marked image, asked for at once and reported in one notice
/// when the last result is in: one notice per image would bury the screen.
#[derive(Debug)]
pub struct Batch {
    pub action: Action,
    pub total: usize,
    /// Asked for, with no result yet.
    waiting: HashSet<PathBuf>,
    pub succeeded: usize,
    /// In the order the results came; they stay marked, for another try.
    pub failed: Vec<(PathBuf, Failure)>,
}

impl Batch {
    pub fn new(action: Action, paths: &[PathBuf]) -> Self {
        Self {
            action,
            total: paths.len(),
            waiting: paths.iter().cloned().collect(),
            succeeded: 0,
            failed: Vec::new(),
        }
    }

    pub fn holds(&self, path: &Path) -> bool {
        self.waiting.contains(path)
    }

    /// Records `path`'s result. Returns whether it was the last.
    pub fn finish(&mut self, path: &Path, result: Result<(), Failure>) -> bool {
        if self.waiting.remove(path) {
            match result {
                Ok(()) => self.succeeded += 1,
                Err(failure) => self.failed.push((path.to_path_buf(), failure)),
            }
        }
        self.waiting.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_batch_ends_with_its_last_result() {
        let paths = [PathBuf::from("a.jpg"), PathBuf::from("b.jpg")];
        let mut batch = Batch::new(Action::Delete(Deletion::ToRecycleBin), &paths);
        assert!(batch.holds(Path::new("a.jpg")));
        assert!(!batch.finish(Path::new("a.jpg"), Ok(())));
        assert!(!batch.holds(Path::new("a.jpg")));
        // A result it never asked for changes nothing.
        assert!(!batch.finish(Path::new("c.jpg"), Ok(())));
        assert!(batch.finish(Path::new("b.jpg"), Err(Failure::Exists)));
        assert_eq!(batch.succeeded, 1);
        assert_eq!(batch.failed, [(PathBuf::from("b.jpg"), Failure::Exists)]);
    }
}
