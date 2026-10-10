//! The slideshow: a timed advance that waits for the next image rather than show `Loading…`.

use std::time::{Duration, Instant};

/// Advances every interval while on. The interval runs from when the image changed, however it
/// changed, so a manual step gets a full interval too.
#[derive(Debug)]
pub struct Slideshow {
    interval: Duration,
    /// When the current interval began; `None` while off.
    since: Option<Instant>,
}

impl Slideshow {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            since: None,
        }
    }

    /// From the preferences screen. The interval running ends at the new length from its start.
    pub fn set_interval(&mut self, interval: Duration) {
        self.interval = interval;
    }

    pub fn is_on(&self) -> bool {
        self.since.is_some()
    }

    /// Returns whether it is now on.
    pub fn toggle(&mut self, now: Instant) -> bool {
        self.since = match self.since {
            Some(_) => None,
            None => Some(now),
        };
        self.is_on()
    }

    /// Returns whether it was on.
    pub fn stop(&mut self) -> bool {
        self.since.take().is_some()
    }

    /// The image changed: the next advance is a full interval away.
    pub fn restart(&mut self, now: Instant) {
        if let Some(since) = &mut self.since {
            *since = now;
        }
    }

    /// When the interval ends. Past it, the advance waits for the next image to be ready.
    pub fn due(&self) -> Option<Instant> {
        self.since.map(|since| since + self.interval)
    }

    /// Whether to advance now. Past the interval, a next image still decoding holds the current
    /// one on screen, so a slow decode lengthens that interval instead of showing `Loading…`;
    /// the caller asks again when a decode finishes. The interval restarts at `now`, not at
    /// `due`, so the image that waited still gets a full interval.
    pub fn tick(&mut self, now: Instant, next_ready: bool) -> bool {
        let advance = next_ready && self.due().is_some_and(|due| now >= due);
        if advance {
            self.restart(now);
        }
        advance
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTERVAL: Duration = Duration::from_secs(10);

    fn secs(s: u64) -> Duration {
        Duration::from_secs(s)
    }

    #[test]
    fn advances_every_interval_while_on() {
        let start = Instant::now();
        let mut show = Slideshow::new(INTERVAL);
        assert!(!show.tick(start + secs(30), true), "off does nothing");
        assert!(show.toggle(start));
        assert!(!show.tick(start + secs(9), true));
        assert!(show.tick(start + secs(10), true));
        assert_eq!(show.due(), Some(start + secs(20)));
        assert!(!show.toggle(start + secs(12)));
        assert_eq!(show.due(), None);
    }

    #[test]
    fn a_manual_step_restarts_the_interval() {
        let start = Instant::now();
        let mut show = Slideshow::new(INTERVAL);
        show.toggle(start);
        show.restart(start + secs(6));
        assert!(!show.tick(start + secs(10), true));
        assert!(show.tick(start + secs(16), true));
    }

    #[test]
    fn a_slow_decode_lengthens_the_interval_and_the_next_one_is_full() {
        let start = Instant::now();
        let mut show = Slideshow::new(INTERVAL);
        show.toggle(start);
        assert!(
            !show.tick(start + secs(10), false),
            "waits for the next image"
        );
        assert!(!show.tick(start + secs(12), false));
        assert!(show.tick(start + secs(13), true));
        assert_eq!(show.due(), Some(start + secs(23)));
    }

    #[test]
    fn restarting_while_off_leaves_it_off() {
        let mut show = Slideshow::new(INTERVAL);
        show.restart(Instant::now());
        assert!(!show.is_on());
        assert!(!show.stop());
    }
}
