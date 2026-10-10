//! The preload cache's policy: which images it wants, in what order, and which it lets go. Generic
//! over what it holds, so the rules are tested without a GPU; `shell` stores textures in it.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::browse::domain::Direction;
use crate::load::domain::DecodeError;

/// Images wanted ahead of the current one in the direction of travel, and behind it.
const AHEAD: usize = 3;
const BEHIND: usize = 1;

/// Image list positions of the preload window, highest priority first: the current image, then
/// `AHEAD` in the direction of travel, then `BEHIND` the other way. Wraps at the ends, as
/// navigation does; a short list names each position once.
pub fn preload_window(len: usize, current: usize, direction: Direction) -> Vec<usize> {
    if len == 0 {
        return Vec::new();
    }
    // Stepping back by `len - 1` is stepping back by one, without going below zero.
    let (ahead, behind) = match direction {
        Direction::Forward => (1, len - 1),
        Direction::Backward => (len - 1, 1),
    };
    let mut window = vec![current];
    let steps = (1..=AHEAD)
        .map(|n| n * ahead)
        .chain((1..=BEHIND).map(|n| n * behind));
    for step in steps {
        let position = (current + step % len) % len;
        if !window.contains(&position) {
            window.push(position);
        }
    }
    window
}

/// Decoded images by path, under a budget. Keyed by path so re-sorting keeps them; each entry
/// also remembers its image list position, which [`PreloadCache::reindex`] refreshes, because
/// eviction measures distance from the current image.
#[derive(Debug)]
pub struct PreloadCache<T> {
    /// Bytes the entries may hold. The preload window is kept even past it.
    budget: u64,
    used: u64,
    entries: HashMap<PathBuf, Entry<T>>,
    /// The preload window, highest priority first; its first path is the current image.
    window: Vec<PathBuf>,
    /// The current image's position and the list length, for distances.
    current: usize,
    len: usize,
}

#[derive(Debug)]
struct Entry<T> {
    /// A failed decode is kept too, so returning to it shows its error card without retrying.
    loaded: Result<T, DecodeError>,
    bytes: u64,
    position: usize,
}

impl<T> PreloadCache<T> {
    pub fn new(budget: u64) -> Self {
        Self {
            budget,
            used: 0,
            entries: HashMap::new(),
            window: Vec::new(),
            current: 0,
            len: 0,
        }
    }

    /// Bytes held, for the budget.
    pub fn used(&self) -> u64 {
        self.used
    }

    pub fn get(&self, path: &Path) -> Option<&Result<T, DecodeError>> {
        self.entries.get(path).map(|entry| &entry.loaded)
    }

    /// Moves the preload window: `window` is its positions and paths, highest priority first, as
    /// [`preload_window`] orders them, over a list of `len` images. Entries that left it become
    /// evictable.
    pub fn retarget(&mut self, len: usize, window: Vec<(usize, PathBuf)>) {
        self.len = len;
        self.current = window.first().map_or(0, |(position, _)| *position);
        self.window = window.into_iter().map(|(_, path)| path).collect();
        self.evict();
    }

    /// What the workers should decode, highest priority first: the preload window less what is
    /// already held.
    pub fn missing(&self) -> Vec<PathBuf> {
        self.window
            .iter()
            .filter(|path| !self.entries.contains_key(*path))
            .cloned()
            .collect()
    }

    /// Whether [`insert`](Self::insert) would keep an image of `bytes`, so a result nobody will
    /// keep is never uploaded. The preload window is always kept; anything else only if it fits
    /// the budget as things stand, since it must not push out a nearer image.
    pub fn keeps(&self, path: &Path, bytes: u64) -> bool {
        !self.entries.contains_key(path)
            && (self.window.iter().any(|wanted| wanted == path) || self.used + bytes <= self.budget)
    }

    /// Adds a decode result at image list `position`, then evicts down to the budget. Returns
    /// whether it was kept.
    pub fn insert(
        &mut self,
        path: PathBuf,
        position: usize,
        bytes: u64,
        loaded: Result<T, DecodeError>,
    ) -> bool {
        if !self.keeps(&path, bytes) {
            return false;
        }
        self.used += bytes;
        self.entries.insert(
            path.clone(),
            Entry {
                loaded,
                bytes,
                position,
            },
        );
        self.evict();
        self.entries.contains_key(&path)
    }

    /// Refreshes positions after the image list changed order or content, from its paths in
    /// display order; entries no longer in it go.
    pub fn reindex<'a>(&mut self, paths: impl IntoIterator<Item = &'a Path>) {
        let mut seen = HashSet::with_capacity(self.entries.len());
        for (position, path) in paths.into_iter().enumerate() {
            if let Some(entry) = self.entries.get_mut(path) {
                entry.position = position;
                seen.insert(path.to_path_buf());
            }
        }
        self.entries.retain(|path, _| seen.contains(path));
        self.used = self.entries.values().map(|entry| entry.bytes).sum();
    }

    /// Over budget, drops the held image furthest from the current one outside the preload
    /// window, until it fits or only the window is left.
    fn evict(&mut self) {
        while self.used > self.budget {
            let furthest = self
                .entries
                .iter()
                .filter(|(path, entry)| entry.bytes > 0 && !self.window.contains(path))
                // The path breaks ties between equal distances ahead and behind, so the choice
                // never depends on hash order.
                .max_by(|(a_path, a), (b_path, b)| {
                    self.distance(a.position)
                        .cmp(&self.distance(b.position))
                        .then_with(|| a_path.cmp(b_path))
                })
                .map(|(path, _)| path.clone());
            let Some(path) = furthest else { return };
            let entry = self.entries.remove(&path).expect("just found");
            self.used -= entry.bytes;
        }
    }

    /// Steps from the current image either way round the list, since navigation wraps.
    fn distance(&self, position: usize) -> usize {
        let apart = position.abs_diff(self.current);
        apart.min(self.len.saturating_sub(apart))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use Direction::{Backward, Forward};

    fn path(position: usize) -> PathBuf {
        PathBuf::from(format!("{position}.jpg"))
    }

    /// A cache over a list of `len` images whose paths are their positions.
    fn cache_at(budget: u64, len: usize, current: usize, direction: Direction) -> PreloadCache<()> {
        let mut cache = PreloadCache::new(budget);
        retarget(&mut cache, len, current, direction);
        cache
    }

    fn retarget(cache: &mut PreloadCache<()>, len: usize, current: usize, direction: Direction) {
        let window = preload_window(len, current, direction)
            .into_iter()
            .map(|position| (position, path(position)))
            .collect();
        cache.retarget(len, window);
    }

    fn held(cache: &PreloadCache<()>) -> Vec<usize> {
        let mut positions: Vec<usize> = cache.entries.values().map(|e| e.position).collect();
        positions.sort_unstable();
        positions
    }

    #[test]
    fn the_window_leans_the_way_of_travel() {
        assert_eq!(preload_window(100, 50, Forward), [50, 51, 52, 53, 49]);
        assert_eq!(preload_window(100, 50, Backward), [50, 49, 48, 47, 51]);
    }

    #[test]
    fn the_window_wraps_at_the_ends_of_the_list() {
        assert_eq!(preload_window(100, 98, Forward), [98, 99, 0, 1, 97]);
        assert_eq!(preload_window(100, 0, Forward), [0, 1, 2, 3, 99]);
        assert_eq!(preload_window(100, 1, Backward), [1, 0, 99, 98, 2]);
    }

    #[test]
    fn a_short_list_names_each_image_once() {
        assert_eq!(preload_window(3, 2, Forward), [2, 0, 1]);
        assert_eq!(preload_window(1, 0, Backward), [0]);
        assert!(preload_window(0, 0, Forward).is_empty());
    }

    #[test]
    fn jobs_come_in_window_order_without_what_is_held() {
        let mut cache = cache_at(1000, 100, 10, Forward);
        assert_eq!(cache.missing(), [10, 11, 12, 13, 9].map(path));
        cache.insert(path(11), 11, 10, Ok(()));
        cache.insert(path(10), 10, 10, Ok(()));
        assert_eq!(cache.missing(), [12, 13, 9].map(path));
    }

    #[test]
    fn over_budget_the_furthest_image_outside_the_window_goes() {
        let mut cache = cache_at(40, 100, 50, Forward);
        // Out of the window: 40 ahead, 10 behind, 30 behind (wrapping is 70 the other way round).
        for position in [90, 40, 20] {
            assert!(cache.insert(path(position), position, 10, Ok(())));
        }
        assert!(cache.insert(path(50), 50, 10, Ok(())));
        assert_eq!(held(&cache), [20, 40, 50, 90]);
        // One more in the window: the furthest outside it, 90 at 40 steps, goes.
        assert!(cache.insert(path(51), 51, 10, Ok(())));
        assert_eq!(held(&cache), [20, 40, 50, 51]);
        assert_eq!(cache.used(), 40);
    }

    #[test]
    fn distance_is_measured_round_the_wrap() {
        let mut cache = cache_at(20, 100, 2, Forward);
        // 97 is 5 steps back round the wrap; 60 is 42 steps away.
        assert!(cache.insert(path(97), 97, 10, Ok(())));
        assert!(cache.insert(path(60), 60, 10, Ok(())));
        assert!(cache.insert(path(2), 2, 10, Ok(())));
        assert_eq!(held(&cache), [2, 97]);
    }

    #[test]
    fn the_window_is_kept_even_past_the_budget() {
        let mut cache = cache_at(25, 100, 0, Forward);
        for position in [0, 1, 2] {
            assert!(cache.insert(path(position), position, 10, Ok(())));
        }
        assert_eq!(cache.used(), 30);
        // Outside the window, an image is kept only if it fits as things stand.
        assert!(!cache.keeps(&path(50), 1));
        assert!(!cache.insert(path(50), 50, 1, Ok(())));
        // Once the window moves on, the old images become evictable and the budget holds again.
        retarget(&mut cache, 100, 10, Forward);
        assert_eq!(cache.used(), 20);
        assert_eq!(held(&cache), [1, 2]);
    }

    #[test]
    fn a_decode_that_finishes_after_leaving_the_window_is_kept_if_it_fits() {
        let mut cache = cache_at(100, 100, 0, Forward);
        retarget(&mut cache, 100, 30, Forward);
        assert!(cache.insert(path(3), 3, 60, Ok(())));
        assert!(!cache.insert(path(4), 4, 60, Ok(())));
        assert_eq!(cache.used(), 60);
    }

    #[test]
    fn failed_decodes_are_kept_and_cost_nothing() {
        let mut cache = cache_at(0, 10, 0, Forward);
        let error = DecodeError::cut_short();
        assert!(cache.insert(path(0), 0, 0, Err(error)));
        assert!(matches!(cache.get(&path(0)), Some(Err(_))));
        assert_eq!(cache.used(), 0);
        assert!(!cache.missing().contains(&path(0)));
    }

    #[test]
    fn reordering_keeps_entries_at_their_new_positions() {
        let mut cache = cache_at(1000, 4, 0, Forward);
        for position in 0..4 {
            cache.insert(path(position), position, 10, Ok(()));
        }
        // Reversed, and 1.jpg is gone from the list.
        let order = [path(3), path(2), path(0)];
        cache.reindex(order.iter().map(PathBuf::as_path));
        assert_eq!(cache.entries[&path(3)].position, 0);
        assert_eq!(cache.entries[&path(0)].position, 2);
        assert!(cache.get(&path(1)).is_none());
        assert_eq!(cache.used(), 30);
    }
}
