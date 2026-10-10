//! Scanning paths into the image list, the folder filter, sort modes, shuffle and grouping.
//! No GPU, and no I/O beyond the filesystem scan.

#![expect(
    dead_code,
    reason = "scaffold: scanning and sorting arrive with the image list"
)]

use std::path::PathBuf;
use std::time::SystemTime;

/// Direction of travel through the image list; the preload window leans this way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Forward,
    Backward,
}

/// What N, D and Z sort by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    /// Natural and case-insensitive: `img2` before `img10`.
    Name,
    /// The file's modified time.
    Date,
    Size,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortMode {
    By(SortKey),
    /// The seed is kept so session restore can rebuild the same order.
    Shuffle {
        seed: u64,
    },
}

/// One image found by a scan. Holds what sorting needs, so re-sorting never touches the disk.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub size: u64,
}
