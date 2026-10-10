//! Decode worker threads behind one priority queue. The main thread replaces the queue whenever
//! the preload window moves, so a job that left the window before a worker reached it is never
//! decoded; one already running finishes, and the cache decides whether to keep it.

use std::collections::{HashSet, VecDeque};
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread;

use crate::load::domain::{DecodeError, Decoded};
use crate::load::infrastructure::decode;

/// Most a decode can use: zune-jpeg and `fast_image_resize` are single-threaded per image, so
/// parallelism comes from decoding several images at once. One core stays free for the event
/// loop and uploads, and more than 4 would decode past the preload window anyway.
const MAX_WORKERS: usize = 4;

/// Dropping it stops the workers once their current decode finishes; their results then have
/// nowhere to go.
pub struct Workers {
    shared: Arc<Shared>,
}

#[derive(Default)]
struct Shared {
    queue: Mutex<Queue>,
    wake: Condvar,
}

/// The jobs waiting and running. Kept apart from the threads so the replacement rules are tested
/// without them.
#[derive(Debug, Default)]
struct Queue {
    /// Highest priority first.
    waiting: VecDeque<PathBuf>,
    /// Taken by a worker, until the main thread has the result: until then a new request for the
    /// same path would decode it twice.
    running: HashSet<PathBuf>,
    closed: bool,
}

impl Queue {
    /// Replaces what waits with `paths`, less what is running.
    fn request(&mut self, paths: Vec<PathBuf>) {
        self.waiting = paths
            .into_iter()
            .filter(|path| !self.running.contains(path))
            .collect();
    }

    fn take(&mut self) -> Option<PathBuf> {
        let path = self.waiting.pop_front()?;
        self.running.insert(path.clone());
        Some(path)
    }
}

impl Workers {
    /// Starts `min(available cores - 1, 4)` workers, at least 1. Each decode downscales to
    /// `max_dim` and is handed to `send` from the worker's thread.
    pub fn spawn<F>(max_dim: u32, send: F) -> Self
    where
        F: Fn(PathBuf, Result<Decoded, DecodeError>) + Clone + Send + 'static,
    {
        let shared = Arc::new(Shared::default());
        let cores = thread::available_parallelism().map_or(1, usize::from);
        let count = cores.saturating_sub(1).clamp(1, MAX_WORKERS);
        for n in 0..count {
            let shared = shared.clone();
            let send = send.clone();
            thread::Builder::new()
                .name(format!("decode {n}"))
                .spawn(move || work(&shared, max_dim, send))
                .expect("the OS starts a thread");
        }
        Self { shared }
    }

    /// The jobs to run, highest priority first, replacing every job not yet started.
    pub fn request(&self, paths: Vec<PathBuf>) {
        self.shared.lock().request(paths);
        self.shared.wake.notify_all();
    }

    /// Called by the main thread when the result for `path` arrives, so it can be requested again.
    pub fn finished(&self, path: &Path) {
        self.shared.lock().running.remove(path);
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        self.shared.lock().closed = true;
        self.shared.wake.notify_all();
    }
}

impl Shared {
    /// A worker that panicked never holds the lock (it decodes unlocked), so poisoning means
    /// nothing here.
    fn lock(&self) -> MutexGuard<'_, Queue> {
        self.queue
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn work<F>(shared: &Shared, max_dim: u32, send: F)
where
    F: Fn(PathBuf, Result<Decoded, DecodeError>),
{
    loop {
        let path = {
            let mut queue = shared.lock();
            loop {
                if queue.closed {
                    return;
                }
                if let Some(path) = queue.take() {
                    break path;
                }
                queue = shared
                    .wake
                    .wait(queue)
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
            }
        };
        // Files are untrusted and the decoders are third-party: a panic on one file must cost
        // that file, not a worker.
        let result = panic::catch_unwind(AssertUnwindSafe(|| decode(&path, max_dim)))
            .unwrap_or_else(|_| Err(DecodeError::damaged("the decoder crashed")));
        send(path, result);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use image::{ImageFormat, RgbaImage};

    use super::*;

    fn paths(names: &[&str]) -> Vec<PathBuf> {
        names.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn a_new_request_drops_waiting_jobs_but_not_running_ones() {
        let mut queue = Queue::default();
        queue.request(paths(&["a", "b", "c"]));
        assert_eq!(queue.take(), Some(PathBuf::from("a")));
        // The window moved: b left it, a is still wanted but already running.
        queue.request(paths(&["a", "d"]));
        assert_eq!(queue.take(), Some(PathBuf::from("d")));
        assert_eq!(queue.take(), None);
        assert_eq!(queue.running.len(), 2);
    }

    #[test]
    fn workers_decode_every_request_including_broken_files() {
        let dir = tempfile::tempdir().unwrap();
        let good = dir.path().join("good.png");
        RgbaImage::new(3, 2)
            .save_with_format(&good, ImageFormat::Png)
            .unwrap();
        let bad = dir.path().join("bad.png");
        std::fs::write(&bad, b"not a png").unwrap();

        let (sender, results) = mpsc::channel();
        let workers = Workers::spawn(16384, move |path, result| {
            sender.send((path, result)).unwrap();
        });
        workers.request(vec![bad.clone(), good.clone()]);
        let mut received: Vec<_> = (0..2)
            .map(|_| results.recv_timeout(Duration::from_secs(10)).unwrap())
            .collect();
        received.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(received[0].0, bad);
        assert!(received[0].1.is_err());
        assert_eq!(received[1].0, good);
        assert_eq!(received[1].1.as_ref().unwrap().size, [3, 2]);
    }
}
