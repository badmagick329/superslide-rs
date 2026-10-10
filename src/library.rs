//! Scanning paths into the image list, the folder filter, sort modes, shuffle and grouping.
//! No GPU, and no I/O beyond the filesystem scan.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{self, AtomicBool};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use walkdir::WalkDir;

use crate::decode::Format;

/// Progress counts go out at most this often; a scan that finishes sooner sends none, so a quick
/// open never flashes a `Scanning…` notice.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(100);

/// Direction of travel through the image list; the preload window leans this way.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[expect(dead_code, reason = "scaffold: navigation arrives with its issue")]
pub enum Direction {
    Forward,
    Backward,
}

/// What N, D and Z sort by.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "scaffold: sort keys arrive with navigation")
)]
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

impl SortMode {
    /// A shuffle no earlier shuffle repeats.
    pub fn new_shuffle() -> Self {
        // `RandomState` keys are random per process and change with every `new`.
        SortMode::Shuffle {
            seed: RandomState::new().hash_one(SystemTime::now()),
        }
    }
}

/// Everything that decides the image list's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Order {
    pub mode: SortMode,
    /// Reverses the order within each group; groups stay in folder-path order.
    pub reversed: bool,
    /// Group by subfolder: groups in natural folder-path order, the sort mode inside each.
    pub grouped: bool,
}

impl Order {
    pub fn new(mode: SortMode) -> Self {
        Self {
            mode,
            reversed: false,
            grouped: false,
        }
    }
}

/// One image found by a scan. Holds what sorting needs, so re-sorting never touches the disk.
#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub modified: SystemTime,
    pub size: u64,
    /// The file name and folder path, lowercased once here: lowercasing and path parsing inside
    /// the comparison made re-sorting 100,000 images take a third of a second.
    name_key: String,
    folder_key: String,
}

impl Entry {
    pub fn new(path: PathBuf, modified: SystemTime, size: u64) -> Self {
        let name_key = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase();
        let folder_key = path
            .parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .to_lowercase();
        Self {
            path,
            modified,
            size,
            name_key,
            folder_key,
        }
    }
}

/// The ordered images from the last open and which one is current.
#[derive(Debug)]
pub struct ImageList {
    /// In display order.
    entries: Vec<Entry>,
    /// Index into `entries`; meaningless while `entries` is empty.
    current: usize,
    order: Order,
}

impl ImageList {
    /// Starts at `current` when it is in the list, else at the first image.
    pub fn new(entries: Vec<Entry>, current: Option<&Path>, order: Order) -> Self {
        let mut list = Self {
            entries,
            current: 0,
            order,
        };
        arrange(&mut list.entries, order);
        list.current = current.and_then(|path| list.position(path)).unwrap_or(0);
        list
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "scaffold: navigation arrives with its issue")
    )]
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "scaffold: navigation arrives with its issue")
    )]
    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.current)
    }

    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "scaffold: sort keys arrive with navigation")
    )]
    pub fn order(&self) -> Order {
        self.order
    }

    /// Re-sorts in memory, keeping the same current image.
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "scaffold: sort keys arrive with navigation")
    )]
    pub fn set_order(&mut self, order: Order) {
        let current = self.current().map(|entry| entry.path.clone());
        self.order = order;
        arrange(&mut self.entries, order);
        self.current = current.and_then(|path| self.position(&path)).unwrap_or(0);
    }

    fn position(&self, path: &Path) -> Option<usize> {
        self.entries.iter().position(|entry| entry.path == path)
    }
}

fn arrange(entries: &mut [Entry], order: Order) {
    match order.mode {
        // Unstable is fine: the path tie-break makes every order total.
        SortMode::By(key) => entries.sort_unstable_by(|a, b| {
            let ordering = compare(key, a, b);
            if order.reversed {
                ordering.reverse()
            } else {
                ordering
            }
        }),
        SortMode::Shuffle { seed } => {
            // Scan order depends on the filesystem; starting from path order makes a seed give
            // the same shuffle every time.
            entries.sort_unstable_by(|a, b| a.path.as_os_str().cmp(b.path.as_os_str()));
            shuffle(entries, seed);
            if order.reversed {
                entries.reverse();
            }
        }
    }
    if order.grouped {
        // Stable, so each group keeps the order just arranged.
        entries.sort_by(|a, b| {
            // The exact comparison breaks natural-order ties between distinct folders, which
            // would otherwise interleave their images.
            natural(&a.folder_key, &b.folder_key).then_with(|| folder(&a.path).cmp(folder(&b.path)))
        });
    }
}

fn compare(key: SortKey, a: &Entry, b: &Entry) -> Ordering {
    let by_name = || {
        natural(&a.name_key, &b.name_key).then_with(|| a.path.as_os_str().cmp(b.path.as_os_str()))
    };
    match key {
        SortKey::Name => by_name(),
        SortKey::Date => a.modified.cmp(&b.modified).then_with(by_name),
        SortKey::Size => a.size.cmp(&b.size).then_with(by_name),
    }
}

fn folder(path: &Path) -> &Path {
    path.parent().unwrap_or(Path::new(""))
}

/// Digit runs compare as numbers: `img2` before `img10`. Case-insensitive because callers pass
/// lowercased text. Equal numbers with different leading zeros compare equal; callers break ties
/// on the path.
fn natural(a: &str, b: &str) -> Ordering {
    // Bytes, not chars: UTF-8 byte order is code point order, and ASCII digits never occur inside
    // a multi-byte character.
    let (mut a, mut b) = (a.as_bytes(), b.as_bytes());
    loop {
        let (Some(&x), Some(&y)) = (a.first(), b.first()) else {
            // At least one is used up: the shorter comes first.
            return a.len().cmp(&b.len());
        };
        if x.is_ascii_digit() && y.is_ascii_digit() {
            let (number_a, rest_a) = split_digits(a);
            let (number_b, rest_b) = split_digits(b);
            // Compared as strings, so a run of any length never overflows.
            let ordering = number_a
                .len()
                .cmp(&number_b.len())
                .then_with(|| number_a.cmp(number_b));
            if ordering.is_ne() {
                return ordering;
            }
            (a, b) = (rest_a, rest_b);
        } else if x != y {
            return x.cmp(&y);
        } else {
            (a, b) = (&a[1..], &b[1..]);
        }
    }
}

/// The leading digit run without its leading zeros, and what follows it.
fn split_digits(text: &[u8]) -> (&[u8], &[u8]) {
    let end = text
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(text.len());
    let (digits, rest) = text.split_at(end);
    let zeros = digits.iter().take_while(|&&digit| digit == b'0').count();
    (&digits[zeros..], rest)
}

/// Fisher–Yates driven by SplitMix64: tiny, and the same on every platform and Rust version,
/// which a session-restored shuffle depends on. The modulo bias is below 1e-12 for any real
/// image list.
fn shuffle<T>(items: &mut [T], seed: u64) {
    let mut state = seed;
    for i in (1..items.len()).rev() {
        let j = (split_mix_64(&mut state) % (i as u64 + 1)) as usize;
        items.swap(i, j);
    }
}

fn split_mix_64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Decided by extension alone: the scan never opens files.
pub fn is_image(path: &Path) -> bool {
    Format::from_path(path).is_some()
}

/// What to scan; one per opening route in `docs/context/architecture.md` *Opening*.
#[derive(Clone, Debug)]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "scaffold: Shift+O arrives with navigation")
)]
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
    /// thread.
    pub fn spawn(
        open: Open,
        folder_filter: &[String],
        mut send: impl FnMut(ScanEvent) + Send + 'static,
    ) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        let cancelled = cancel.clone();
        let folder_filter = folder_filter.to_vec();
        thread::Builder::new()
            .name("scan".to_owned())
            .spawn(move || {
                let found = scan(&open, &folder_filter, &cancelled, |count| {
                    send(ScanEvent::Progress(count))
                });
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
    let (current, order) = match open {
        Open::Paths(paths) => {
            let paths: Vec<PathBuf> = paths.iter().map(|path| absolute(path)).collect();
            let current = match paths.as_slice() {
                [path] if !path.is_dir() => {
                    // A lone file opens its whole folder; the file itself is checked here so a
                    // mistyped path reports as missing rather than opening its parent.
                    if walker.check(path) {
                        walker.add_folder(folder(path), false)?;
                    }
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
            (current, Order::new(SortMode::By(SortKey::Name)))
        }
        Open::Recursive(root) => {
            let root = absolute(root);
            if walker.check(&root) {
                walker.add_folder(&root, true)?;
            }
            (None, Order::new(SortMode::new_shuffle()))
        }
    };
    Some(Found {
        list: ImageList::new(walker.entries, current.as_deref(), order),
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
    use std::time::Duration;

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
        scan(&open, &filter, &AtomicBool::new(false), |_| {}).unwrap()
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
    fn only_the_five_extensions_are_images_in_any_case() {
        for name in ["a.jpg", "a.JPEG", "a.Png", "a.tga", "a.BMP"] {
            assert!(is_image(Path::new(name)), "{name}");
        }
        for name in ["a.gif", "a.webp", "a.jpg.txt", "jpg", "a."] {
            assert!(!is_image(Path::new(name)), "{name}");
        }
    }

    #[test]
    fn a_file_opens_its_folder_at_that_file_without_subfolders() {
        let root = tree(&["b.jpg", "a.png", "c.txt", "sub/d.jpg"]);
        let found = open(Open::Paths(vec![root.path().join("b.jpg")]), &[]);
        assert_eq!(names(&found.list, root.path()), ["a.png", "b.jpg"]);
        assert_eq!(current(&found.list, root.path()), "b.jpg");
        assert_eq!(found.list.order(), Order::new(SortMode::By(SortKey::Name)));
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
        assert!(scan(&open, &[], &cancel, |_| {}).is_none());
    }

    fn entry(path: &str, modified_s: u64, size: u64) -> Entry {
        Entry::new(
            PathBuf::from(path),
            SystemTime::UNIX_EPOCH + Duration::from_secs(modified_s),
            size,
        )
    }

    fn paths(list: &ImageList) -> Vec<&str> {
        list.entries()
            .iter()
            .map(|entry| entry.path.to_str().unwrap())
            .collect()
    }

    fn sample() -> Vec<Entry> {
        vec![
            entry("f/img10.jpg", 3, 10),
            entry("f/IMG2.jpg", 1, 30),
            entry("f/img1.jpg", 2, 20),
            entry("f/b.jpg", 4, 20),
        ]
    }

    fn by(key: SortKey) -> Order {
        Order::new(SortMode::By(key))
    }

    #[test]
    fn name_sort_is_natural_and_case_insensitive() {
        let list = ImageList::new(sample(), None, by(SortKey::Name));
        assert_eq!(
            paths(&list),
            ["f/b.jpg", "f/img1.jpg", "f/IMG2.jpg", "f/img10.jpg"]
        );
        assert_eq!(natural("a01", "a1"), Ordering::Equal);
        assert_eq!(natural("a", "a1"), Ordering::Less);
        // Longer than any integer type.
        assert_eq!(
            natural("x99999999999999999999999", "x100000000000000000000000"),
            Ordering::Less
        );
        let accented = ImageList::new(
            vec![entry("f/écran2.jpg", 0, 0), entry("f/Écran.jpg", 0, 0)],
            None,
            by(SortKey::Name),
        );
        assert_eq!(paths(&accented), ["f/Écran.jpg", "f/écran2.jpg"]);
    }

    #[test]
    fn date_and_size_sort_break_ties_by_name() {
        let date = ImageList::new(sample(), None, by(SortKey::Date));
        assert_eq!(
            paths(&date),
            ["f/IMG2.jpg", "f/img1.jpg", "f/img10.jpg", "f/b.jpg"]
        );
        let size = ImageList::new(sample(), None, by(SortKey::Size));
        assert_eq!(
            paths(&size),
            ["f/img10.jpg", "f/b.jpg", "f/img1.jpg", "f/IMG2.jpg"]
        );
    }

    #[test]
    fn reverse_flips_the_sort() {
        let order = Order {
            reversed: true,
            ..by(SortKey::Name)
        };
        let list = ImageList::new(sample(), None, order);
        assert_eq!(
            paths(&list),
            ["f/img10.jpg", "f/IMG2.jpg", "f/img1.jpg", "f/b.jpg"]
        );
    }

    #[test]
    fn a_shuffle_repeats_from_its_seed_whatever_the_scan_order() {
        let many: Vec<Entry> = (0..50)
            .map(|i| entry(&format!("f/{i}.jpg"), 0, 0))
            .collect();
        let mut backwards = many.clone();
        backwards.reverse();
        let shuffled = |entries, seed| {
            let list = ImageList::new(entries, None, Order::new(SortMode::Shuffle { seed }));
            let order: Vec<String> = paths(&list).iter().map(|path| path.to_string()).collect();
            order
        };
        let first = shuffled(many.clone(), 7);
        assert_eq!(first, shuffled(backwards, 7));
        assert_ne!(first, shuffled(many.clone(), 8));
        let mut every = first.clone();
        every.sort();
        let mut expected: Vec<String> = many
            .iter()
            .map(|e| e.path.to_str().unwrap().to_owned())
            .collect();
        expected.sort();
        assert_eq!(every, expected, "a shuffle keeps every image once");
        // Pinned: a change to the generator would scramble every restored session.
        assert_eq!(&first[..3], PINNED_SHUFFLE);
        assert!(matches!(SortMode::new_shuffle(), SortMode::Shuffle { seed } if seed != 7));
        assert_ne!(SortMode::new_shuffle(), SortMode::new_shuffle());
    }

    const PINNED_SHUFFLE: [&str; 3] = ["f/3.jpg", "f/23.jpg", "f/9.jpg"];

    #[test]
    fn grouping_orders_folders_naturally_and_sorts_inside_each() {
        let entries = vec![
            entry("r/a10/x.jpg", 0, 1),
            entry("r/a2/y.jpg", 0, 2),
            entry("r/b.jpg", 0, 3),
            entry("r/a2/x.jpg", 0, 4),
            entry("r/a10/w.jpg", 0, 5),
        ];
        let order = Order {
            grouped: true,
            ..by(SortKey::Name)
        };
        let list = ImageList::new(entries.clone(), None, order);
        assert_eq!(
            paths(&list),
            [
                "r/b.jpg",
                "r/a2/x.jpg",
                "r/a2/y.jpg",
                "r/a10/w.jpg",
                "r/a10/x.jpg"
            ]
        );
        // Reverse turns each group round; the groups keep their order.
        let reversed = Order {
            reversed: true,
            ..order
        };
        assert_eq!(
            paths(&ImageList::new(entries.clone(), None, reversed)),
            [
                "r/b.jpg",
                "r/a2/y.jpg",
                "r/a2/x.jpg",
                "r/a10/x.jpg",
                "r/a10/w.jpg"
            ]
        );
        // A grouped shuffle keeps each group together.
        let shuffled = Order {
            mode: SortMode::Shuffle { seed: 1 },
            ..order
        };
        let folders: Vec<String> = paths(&ImageList::new(entries, None, shuffled))
            .iter()
            .map(|path| path.rsplit_once('/').unwrap().0.to_owned())
            .collect();
        assert_eq!(folders, ["r", "r/a2", "r/a2", "r/a10", "r/a10"]);
    }

    #[test]
    fn re_sorting_keeps_the_current_image() {
        let current = Path::new("f/img1.jpg");
        let mut list = ImageList::new(sample(), Some(current), by(SortKey::Name));
        assert_eq!(list.current().unwrap().path, current);
        for order in [
            by(SortKey::Date),
            by(SortKey::Size),
            Order::new(SortMode::Shuffle { seed: 3 }),
            Order {
                reversed: true,
                grouped: true,
                ..by(SortKey::Name)
            },
        ] {
            list.set_order(order);
            assert_eq!(list.order(), order);
            assert_eq!(list.current().unwrap().path, current, "{order:?}");
        }
    }
}
