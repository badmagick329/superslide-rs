//! The image list and its order: sort modes, natural name order, shuffle and grouping. Works on
//! what a scan recorded, so re-sorting never touches the disk.

use std::cmp::Ordering;
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub mod advance;
pub mod slideshow;

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

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.current)
    }

    /// Position of the current image; meaningless while the list is empty.
    pub fn current_position(&self) -> usize {
        self.current
    }

    /// Moves the current image one step, wrapping from the last image to the first and back.
    /// Returns whether it wrapped.
    pub fn step(&mut self, direction: Direction) -> bool {
        let Some((next, wrapped)) = self.neighbour(direction) else {
            return false;
        };
        self.current = next;
        wrapped
    }

    /// The image one step away, which [`Self::step`] would make current.
    pub fn adjacent(&self, direction: Direction) -> Option<&Entry> {
        self.neighbour(direction)
            .map(|(position, _)| &self.entries[position])
    }

    /// Position one step away and whether reaching it wraps; `None` while empty.
    fn neighbour(&self, direction: Direction) -> Option<(usize, bool)> {
        let len = self.entries.len();
        if len == 0 {
            return None;
        }
        Some(match direction {
            Direction::Forward if self.current + 1 == len => (0, true),
            Direction::Forward => (self.current + 1, false),
            Direction::Backward if self.current == 0 => (len - 1, true),
            Direction::Backward => (self.current - 1, false),
        })
    }

    /// Moves to the first image of the next or previous group, wrapping at the ends; previous
    /// means the group before the current image's, wherever in its group that image is. `None`
    /// unless grouped by subfolder. Otherwise returns whether it wrapped.
    pub fn jump_group(&mut self, direction: Direction) -> Option<bool> {
        if !self.order.grouped || self.entries.is_empty() {
            return None;
        }
        let group = |position: usize| folder(&self.entries[position].path);
        let start_of = |mut position: usize| {
            while position > 0 && group(position - 1) == group(position) {
                position -= 1;
            }
            position
        };
        let (next, wrapped) = match direction {
            Direction::Forward => {
                let mut next = self.current;
                while next < self.entries.len() && group(next) == group(self.current) {
                    next += 1;
                }
                match next == self.entries.len() {
                    true => (0, true),
                    false => (next, false),
                }
            }
            Direction::Backward => match start_of(self.current) {
                0 => (start_of(self.entries.len() - 1), true),
                start => (start_of(start - 1), false),
            },
        };
        self.current = next;
        Some(wrapped)
    }

    pub fn order(&self) -> Order {
        self.order
    }

    /// Re-sorts in memory, keeping the same current image.
    pub fn set_order(&mut self, order: Order) {
        let current = self.current().map(|entry| entry.path.clone());
        self.order = order;
        arrange(&mut self.entries, order);
        self.current = current.and_then(|path| self.position(&path)).unwrap_or(0);
    }

    /// A linear search: fine per decode result, too slow per cache entry.
    pub fn position(&self, path: &Path) -> Option<usize> {
        self.entries.iter().position(|entry| entry.path == path)
    }

    /// Takes out an image whose file was deleted or moved. The current image stays current;
    /// if it is the one removed, the next becomes current, wrapping from the last to the first.
    /// `None` when `path` is not in the list.
    pub fn remove(&mut self, path: &Path) -> Option<Removed> {
        let position = self.position(path)?;
        self.entries.remove(position);
        let was_current = position == self.current;
        let mut wrapped = false;
        if position < self.current {
            self.current -= 1;
        } else if was_current && self.current == self.entries.len() {
            self.current = 0;
            wrapped = !self.entries.is_empty();
        }
        Some(Removed {
            was_current,
            wrapped,
        })
    }
}

/// What [`ImageList::remove`] did to the current image.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Removed {
    /// The removed image was current, so the next one now is.
    pub was_current: bool,
    /// Its next image was the first.
    pub wrapped: bool,
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

/// The folder holding `path`; empty for a bare file name.
pub fn folder(path: &Path) -> &Path {
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

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

    #[test]
    fn stepping_wraps_at_both_ends() {
        // Name order: b, img1, IMG2, img10.
        let mut list = ImageList::new(sample(), Some(Path::new("f/IMG2.jpg")), by(SortKey::Name));
        assert!(!list.step(Direction::Forward));
        assert_eq!(list.current().unwrap().path, Path::new("f/img10.jpg"));
        // The slideshow looks one step ahead before taking it.
        let ahead = list.adjacent(Direction::Forward).unwrap().path.clone();
        assert!(list.step(Direction::Forward));
        assert_eq!(list.current().unwrap().path, ahead);
        assert_eq!(list.current_position(), 0);
        assert!(list.step(Direction::Backward));
        assert_eq!(list.current_position(), 3);

        let mut empty = ImageList::new(Vec::new(), None, by(SortKey::Name));
        assert!(!empty.step(Direction::Forward));
        assert!(empty.adjacent(Direction::Forward).is_none());
        assert!(empty.current().is_none());
    }

    #[test]
    fn group_jumps_land_on_first_images_and_wrap() {
        let entries = vec![
            entry("r/a/1.jpg", 0, 0),
            entry("r/a/2.jpg", 0, 0),
            entry("r/b/1.jpg", 0, 0),
            entry("r/c/1.jpg", 0, 0),
            entry("r/c/2.jpg", 0, 0),
        ];
        let order = Order {
            grouped: true,
            ..by(SortKey::Name)
        };
        let at = |path: &str| ImageList::new(entries.clone(), Some(Path::new(path)), order);
        let jumped = |path: &str, direction| {
            let mut list = at(path);
            let wrapped = list.jump_group(direction).unwrap();
            (
                list.current().unwrap().path.to_str().unwrap().to_owned(),
                wrapped,
            )
        };
        assert_eq!(
            jumped("r/a/2.jpg", Direction::Forward),
            ("r/b/1.jpg".into(), false)
        );
        assert_eq!(
            jumped("r/c/1.jpg", Direction::Forward),
            ("r/a/1.jpg".into(), true)
        );
        // From the middle of a group, back goes to the group before, not this group's start.
        assert_eq!(
            jumped("r/c/2.jpg", Direction::Backward),
            ("r/b/1.jpg".into(), false)
        );
        assert_eq!(
            jumped("r/b/1.jpg", Direction::Backward),
            ("r/a/1.jpg".into(), false)
        );
        assert_eq!(
            jumped("r/a/2.jpg", Direction::Backward),
            ("r/c/1.jpg".into(), true)
        );

        let mut ungrouped = ImageList::new(entries, None, by(SortKey::Name));
        assert_eq!(ungrouped.jump_group(Direction::Forward), None);
        assert_eq!(ungrouped.current_position(), 0);
    }

    #[test]
    fn removing_an_image_keeps_or_advances_the_current_one() {
        // Name order: b, img1, IMG2, img10.
        let current = |list: &ImageList| list.current().unwrap().path.to_str().unwrap().to_owned();
        let mut list = ImageList::new(sample(), Some(Path::new("f/IMG2.jpg")), by(SortKey::Name));
        let kept = Removed {
            was_current: false,
            wrapped: false,
        };
        // Before and after the current image: it stays current.
        assert_eq!(list.remove(Path::new("f/b.jpg")), Some(kept));
        assert_eq!(current(&list), "f/IMG2.jpg");
        assert_eq!(list.remove(Path::new("f/img10.jpg")), Some(kept));
        assert_eq!(current(&list), "f/IMG2.jpg");
        assert_eq!(list.remove(Path::new("f/b.jpg")), None);

        // The current image, last in the list: the next is the first.
        let removed = list.remove(Path::new("f/IMG2.jpg")).unwrap();
        assert!(removed.was_current && removed.wrapped);
        assert_eq!(current(&list), "f/img1.jpg");

        let mut middle = ImageList::new(sample(), Some(Path::new("f/img1.jpg")), by(SortKey::Name));
        let removed = middle.remove(Path::new("f/img1.jpg")).unwrap();
        assert!(removed.was_current && !removed.wrapped);
        assert_eq!(current(&middle), "f/IMG2.jpg");

        // The last image: an empty list, with nothing to wrap to.
        let removed = list.remove(Path::new("f/img1.jpg")).unwrap();
        assert!(removed.was_current && !removed.wrapped);
        assert!(list.current().is_none());
    }
}
