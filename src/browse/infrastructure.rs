//! Scanning paths into the image list on its own thread: the opening routes, the folder filter and
//! progress counts. The only filesystem access browsing makes.

use std::collections::HashSet;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use walkdir::WalkDir;

use crate::browse::domain::{Entry, ImageList, Order, Resume, SortKey, SortMode, folder};
use crate::image_file::domain::Format;

/// Progress counts go out at most this often; a scan that finishes sooner sends none, so a quick
/// open never flashes a `Scanning…` notice.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Decided by extension alone: the scan never opens files.
pub fn is_image(path: &Path) -> bool {
    Format::from_path(path).is_some()
}

/// What to scan; one per opening route in `docs/context/architecture.md` *Opening*.
#[derive(Clone, Debug, PartialEq)]
pub enum Open {
    /// Command-line paths or one burst of drops. A lone file opens its folder at that file; a
    /// lone folder opens its images; several paths open the union of each file and each folder's
    /// images. Never recursive.
    Paths(Vec<PathBuf>),
    /// The folder and every subfolder the folder filter allows; shuffled.
    Recursive(PathBuf),
}

/// A finished scan.
#[derive(Debug)]
pub struct Found {
    pub list: ImageList,
    /// What to scan to rebuild this list: the paths made absolute, and a lone file replaced by
    /// its folder, so the list still opens once that file is gone.
    pub opened: Open,
    /// Folders whose listing failed, skipped.
    pub unreadable_folders: usize,
    /// Paths given to open that do not exist or cannot be read.
    pub missing: Vec<PathBuf>,
}

pub enum ScanEvent {
    /// Images found so far.
    Progress(usize),
    Done(Found),
}

/// A scan running on its own thread. Dropping it cancels the scan, so a newer open simply
/// replaces the older one.
pub struct Scan {
    cancel: Arc<AtomicBool>,
}

impl Drop for Scan {
    fn drop(&mut self) {
        self.cancel.store(true, atomic::Ordering::Relaxed);
    }
}

impl Scan {
    /// `send` receives progress, then the result; nothing once cancelled. It runs on the scan
    /// thread. With `resume`, the list takes its order and current image instead of the route's.
    pub fn spawn(
        open: Open,
        folder_filter: &[String],
        resume: Option<Resume>,
        mut send: impl FnMut(ScanEvent) + Send + 'static,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let folder_filter = folder_filter.to_vec();
        thread::Builder::new()
            .name("scan".to_owned())
            .spawn(move || {
                let found = scan(
                    &open,
                    &folder_filter,
                    resume.as_ref(),
                    &cancelled,
                    |count| send(ScanEvent::Progress(count)),
                );
                if let Some(found) = found {
                    send(ScanEvent::Done(found));
                }
            })
            .expect("the OS can start a thread");
        Self { cancel }
    }
}

/// Builds the image list for `open`; `None` once `cancel` is set. Calls `progress` with the
/// images found so far, at most every `PROGRESS_INTERVAL`.
pub fn scan(
    open: &Open,
    folder_filter: &[String],
    resume: Option<&Resume>,
    cancel: &AtomicBool,
    progress: impl FnMut(usize),
) -> Option<Found> {
    let mut walker = Walker {
        folder_filter: folder_filter
            .iter()
            .map(|name| name.to_lowercase())
            .collect(),
        cancel,
        progress,
        last_progress: Instant::now(),
        entries: Vec::new(),
        seen: HashSet::new(),
        unreadable_folders: 0,
        missing: Vec::new(),
    };
    let (current, order, opened) = match open {
        Open::Paths(paths) => {
            let paths: Vec<PathBuf> = paths.iter().map(|path| absolute(path)).collect();
            let mut opened = Open::Paths(paths.clone());
            let current = match paths.as_slice() {
                [path] if !path.is_dir() => {
                    // A lone file opens its whole folder; the file itself is checked here so a
                    // mistyped path reports as missing rather than opening its parent.
                    if walker.check(path) {
                        walker.add_folder(folder(path), false)?;
                    }
                    opened = Open::Paths(vec![folder(path).to_owned()]);
                    Some(path.clone())
                }
                _ => {
                    for path in &paths {
                        if path.is_dir() {
                            walker.add_folder(path, false)?;
                        } else if walker.check(path) && is_image(path) {
                            walker.add_file(path);
                        }
                    }
                    paths.iter().find(|path| is_image(path)).cloned()
                }
            };
            (current, Order::new(SortMode::By(SortKey::Name)), opened)
        }
        Open::Recursive(root) => {
            let root = absolute(root);
            if walker.check(&root) {
                walker.add_folder(&root, true)?;
            }
            let order = Order::new(SortMode::new_shuffle());
            (None, order, Open::Recursive(root))
        }
    };
    let list = match resume {
        Some(resume) => ImageList::resumed(walker.entries, resume),
        None => ImageList::new(walker.entries, current.as_deref(), order),
    };
    Some(Found {
        list,
        opened,
        unreadable_folders: walker.unreadable_folders,
        missing: walker.missing,
    })
}

/// Relative command-line paths resolve against the working directory; an unresolvable one stays
/// as given and reports as missing.
fn absolute(path: &Path) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| path.to_owned())
}

struct Walker<'a, P> {
    /// Lowercased.
    folder_filter: Vec<String>,
    cancel: &'a AtomicBool,
    progress: P,
    last_progress: Instant,
    entries: Vec<Entry>,
    /// Several dropped paths can name the same file twice (a file and its folder).
    seen: HashSet<PathBuf>,
    unreadable_folders: usize,
    missing: Vec<PathBuf>,
}

impl<P: FnMut(usize)> Walker<'_, P> {
    /// Whether a path given to open exists; records it as missing if not.
    fn check(&mut self, path: &Path) -> bool {
        let exists = path.metadata().is_ok();
        if !exists {
            self.missing.push(path.to_owned());
        }
        exists
    }

    /// `None` once cancelled.
    fn add_folder(&mut self, root: &Path, recursive: bool) -> Option<()> {
        let max_depth = if recursive { usize::MAX } else { 1 };
        // A copy: the filter closure would otherwise hold `self` borrowed for the whole walk.
        let folder_filter = self.folder_filter.clone();
        let walk = WalkDir::new(root)
            .min_depth(1)
            .max_depth(max_depth)
            .into_iter()
            // The opened root sits at depth 0, so it is never filtered.
            .filter_entry(|entry| {
                !(entry.depth() > 0
                    && entry.file_type().is_dir()
                    && filtered(&folder_filter, entry.file_name()))
            });
        for item in walk {
            if self.cancel.load(atomic::Ordering::Relaxed) {
                return None;
            }
            match item {
                Ok(entry) if entry.file_type().is_file() && is_image(entry.path()) => {
                    // On Windows the listing already carries the metadata, so this neither touches
                    // the file nor fails in practice; a failure skips the file.
                    if let Ok(metadata) = entry.metadata() {
                        self.push(Entry::new(
                            entry.into_path(),
                            metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                            metadata.len(),
                        ));
                    }
                }
                Ok(_) => {}
                // Walkdir errors are folders it could not list (or a root that vanished).
                Err(_) => self.unreadable_folders += 1,
            }
        }
        Some(())
    }

    fn add_file(&mut self, path: &Path) {
        match path.metadata() {
            Ok(metadata) => self.push(Entry::new(
                path.to_owned(),
                metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH),
                metadata.len(),
            )),
            Err(_) => self.missing.push(path.to_owned()),
        }
    }

    fn push(&mut self, entry: Entry) {
        if !self.seen.insert(entry.path.clone()) {
            return;
        }
        self.entries.push(entry);
        if self.last_progress.elapsed() >= PROGRESS_INTERVAL {
            self.last_progress = Instant::now();
            (self.progress)(self.entries.len());
        }
    }
}

/// `folder_filter` is lowercased.
fn filtered(folder_filter: &[String], name: &OsStr) -> bool {
    let name = name.to_string_lossy().to_lowercase();
    folder_filter.contains(&name)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    /// Files are created empty unless the name has a size after a colon: `"a.jpg:3"`.
    fn tree(files: &[&str]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for file in files {
            let (name, size) = file.split_once(':').unwrap_or((file, "0"));
            let path = root.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, vec![0; size.parse().unwrap()]).unwrap();
        }
        root
    }

    fn open(open: Open, filter: &[&str]) -> Found {
        let filter: Vec<String> = filter.iter().map(|name| name.to_string()).collect();
        scan(&open, &filter, None, &AtomicBool::new(false), |_| {}).unwrap()
    }

    fn relative(path: &Path, root: &Path) -> String {
        let relative = path.strip_prefix(root).unwrap();
        relative.to_string_lossy().replace('\\', "/")
    }

    /// Paths relative to `root`, with `/` separators, in list order.
    fn names(list: &ImageList, root: &Path) -> Vec<String> {
        list.entries()
            .iter()
            .map(|entry| relative(&entry.path, root))
            .collect()
    }

    fn current(list: &ImageList, root: &Path) -> String {
        relative(&list.current().unwrap().path, root)
    }

    #[test]
    fn a_file_opens_its_folder_at_that_file_without_subfolders() {
        let root = tree(&["b.jpg", "a.png", "c.txt", "sub/d.jpg"]);
        let found = open(Open::Paths(vec![root.path().join("b.jpg")]), &[]);
        assert_eq!(names(&found.list, root.path()), ["a.png", "b.jpg"]);
        assert_eq!(current(&found.list, root.path()), "b.jpg");
        assert_eq!(found.list.order(), Order::new(SortMode::By(SortKey::Name)));
        // Its folder is what reopens it.
        assert_eq!(found.opened, Open::Paths(vec![root.path().to_owned()]));
    }

    #[test]
    fn a_resumed_scan_takes_the_saved_order_and_image() {
        let root = tree(&["a.jpg", "b.jpg", "c.jpg"]);
        let resume = Resume {
            order: Order {
                reversed: true,
                ..Order::new(SortMode::By(SortKey::Name))
            },
            current: Some(root.path().join("b.jpg")),
            position: 0,
        };
        let open = Open::Recursive(root.path().to_owned());
        let found = scan(&open, &[], Some(&resume), &AtomicBool::new(false), |_| {}).unwrap();
        assert_eq!(names(&found.list, root.path()), ["c.jpg", "b.jpg", "a.jpg"]);
        assert_eq!(current(&found.list, root.path()), "b.jpg");
    }

    #[test]
    fn a_folder_opens_its_images_at_the_first() {
        let root = tree(&["b.jpg", "a.png", "sub/d.jpg"]);
        let found = open(Open::Paths(vec![root.path().to_owned()]), &[]);
        assert_eq!(names(&found.list, root.path()), ["a.png", "b.jpg"]);
        assert_eq!(current(&found.list, root.path()), "a.png");
    }

    #[test]
    fn several_paths_open_their_union_once_each() {
        let root = tree(&[
            "x/a.jpg",
            "x/b.jpg",
            "y/c.jpg",
            "y/z/d.jpg",
            "e.jpg",
            "f.txt",
        ]);
        let paths = ["y/c.jpg", "x", "x/b.jpg", "f.txt", "e.jpg"];
        let found = open(
            Open::Paths(paths.iter().map(|path| root.path().join(path)).collect()),
            &[],
        );
        assert_eq!(
            names(&found.list, root.path()),
            ["x/a.jpg", "x/b.jpg", "y/c.jpg", "e.jpg"]
        );
        // The first image named, not the first in order.
        assert_eq!(current(&found.list, root.path()), "y/c.jpg");
    }

    #[test]
    fn missing_paths_are_reported_not_fatal() {
        let root = tree(&["a.jpg"]);
        let gone = root.path().join("gone.jpg");
        let found = open(Open::Paths(vec![gone.clone()]), &[]);
        assert_eq!(found.list.len(), 0);
        assert_eq!(found.missing, std::slice::from_ref(&gone));

        let found = open(
            Open::Paths(vec![gone.clone(), root.path().join("a.jpg")]),
            &[],
        );
        assert_eq!(names(&found.list, root.path()), ["a.jpg"]);
        assert_eq!(found.missing, [gone]);
    }

    #[test]
    fn recursive_open_skips_filtered_folders_below_the_root_only() {
        let root = tree(&[
            "Private/a.jpg",
            "Private/keep/b.jpg",
            "Private/deep/PRIVATE/c.jpg",
            "Private/deep/d.jpg",
            "Private/private/e.jpg",
            "Private/privately/f.jpg",
        ]);
        // The opened root is itself named like a filtered folder.
        let opened = root.path().join("Private");
        let found = open(Open::Recursive(opened.clone()), &["private", "Keep"]);
        let mut found_names = names(&found.list, &opened);
        found_names.sort();
        assert_eq!(found_names, ["a.jpg", "deep/d.jpg", "privately/f.jpg"]);
        assert!(matches!(found.list.order().mode, SortMode::Shuffle { .. }));
        assert_eq!(found.unreadable_folders, 0);
    }

    #[test]
    fn a_cancelled_scan_returns_nothing() {
        let root = tree(&["a.jpg"]);
        let cancel = AtomicBool::new(true);
        let open = Open::Recursive(root.path().to_owned());
        assert!(scan(&open, &[], None, &cancel, |_| {}).is_none());
    }
}
