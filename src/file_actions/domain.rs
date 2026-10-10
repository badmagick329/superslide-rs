//! What Delete, M and C ask for and how each can end. No filesystem access.

use std::fmt;
use std::path::PathBuf;

/// M moves and C copies the current image to a folder. Neither overwrites an existing file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    Move,
    Copy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// To the Recycle Bin, with no confirmation: the Recycle Bin is the undo.
    Delete,
    /// Into this folder, under the same file name.
    Transfer(Transfer, PathBuf),
}

impl Action {
    /// Whether success means the file is gone from where it was, so it leaves the image list.
    pub fn removes(&self) -> bool {
        matches!(self, Action::Delete | Action::Transfer(Transfer::Move, _))
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
