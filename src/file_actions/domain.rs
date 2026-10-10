#![expect(dead_code, reason = "scaffold: file actions arrive in phase 2")]

/// M moves and C copies the current image to a folder. Neither overwrites an existing file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Transfer {
    Move,
    Copy,
}
