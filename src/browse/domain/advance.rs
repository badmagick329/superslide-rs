//! The advance limit: how often a held key may change the image.

use std::time::{Duration, Instant};

use crate::browse::domain::{Direction, Travel};

/// Paces held keys that change the image: Space and Backspace, and Vim's j, k, Ctrl+D and Ctrl+U. Key repeat arrives at the keyboard's rate, about 30 a second;
/// a repeat only shows the key is still down, and the image changes at most once per limit. A
/// repeat that comes early is kept and advances the moment the limit passes, so the pace is the
/// limit itself, not the limit rounded up to the next repeat.
#[derive(Debug)]
pub struct AdvanceLimit {
    limit: Duration,
    /// When the image last changed through this limit.
    last: Option<Instant>,
    /// An early repeat waiting for the limit. Only a repeat sets it, so a key whose release was
    /// never seen advances at most once more.
    pending: Option<Travel>,
}

impl AdvanceLimit {
    pub fn new(limit: Duration) -> Self {
        Self {
            limit,
            last: None,
            pending: None,
        }
    }

    /// From the preferences screen. A repeat already waiting advances at the new limit.
    pub fn set_limit(&mut self, limit: Duration) {
        self.limit = limit;
    }

    /// A press or click always advances; the caller advances, this only starts the clock.
    pub fn pressed(&mut self, now: Instant) {
        self.last = Some(now);
        self.pending = None;
    }

    /// A key repeat: true when it advances now; otherwise it waits for [`Self::due`].
    pub fn repeated(&mut self, travel: Travel, now: Instant) -> bool {
        match self.last {
            Some(last) if now < last + self.limit => {
                self.pending = Some(travel);
                false
            }
            _ => {
                self.pressed(now);
                true
            }
        }
    }

    /// The key that was repeating is up: a waiting repeat must not advance after it.
    pub fn released(&mut self, direction: Direction) {
        self.pending
            .take_if(|pending| pending.direction() == direction);
    }

    /// When a waiting repeat advances; the event loop sleeps until then.
    pub fn due(&self) -> Option<Instant> {
        self.pending.and(self.last).map(|last| last + self.limit)
    }

    /// The move to make, once a waiting repeat is due.
    pub fn tick(&mut self, now: Instant) -> Option<Travel> {
        let due = self.due().filter(|&due| now >= due)?;
        // The clock restarts at `due`, not `now`: a late wake-up must not slow the pace.
        self.last = Some(due);
        self.pending.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMIT: Duration = Duration::from_millis(500);
    const FORWARD: Travel = Travel::Step(Direction::Forward);

    /// Holds a key from `start` for `held`, with the keyboard's repeat delay and rate, waking
    /// at `due` as the event loop would. Returns the times the image changed.
    fn hold(limit: &mut AdvanceLimit, start: Instant, held: Duration) -> Vec<Instant> {
        let (delay, rate) = (Duration::from_millis(250), Duration::from_millis(33));
        let mut advances = vec![start];
        limit.pressed(start);
        let mut repeat = start + delay;
        while repeat < start + held {
            while let Some(due) = limit.due().filter(|&due| due <= repeat) {
                // The wake-up runs a little late, as `WaitUntil` does.
                let woke = due + Duration::from_millis(4);
                assert_eq!(limit.tick(woke), Some(FORWARD));
                advances.push(woke);
            }
            if limit.repeated(FORWARD, repeat) {
                advances.push(repeat);
            }
            repeat += rate;
        }
        limit.released(Direction::Forward);
        advances
    }

    #[test]
    fn holding_for_ten_seconds_advances_twenty_times_and_never_faster() {
        let mut limit = AdvanceLimit::new(LIMIT);
        let start = Instant::now();
        let advances = hold(&mut limit, start, Duration::from_secs(10));
        assert_eq!(advances.len(), 20);
        for pair in advances.windows(2) {
            assert!(pair[1] - pair[0] >= LIMIT - Duration::from_millis(5));
        }
        // Let go: nothing more, however long the loop waits.
        assert_eq!(limit.due(), None);
        assert_eq!(limit.tick(start + Duration::from_secs(60)), None);
    }

    #[test]
    fn a_new_limit_paces_the_next_advance() {
        let mut limit = AdvanceLimit::new(LIMIT);
        let start = Instant::now();
        let held = Duration::from_secs(10);
        assert_eq!(hold(&mut limit, start, held).len(), 20);
        limit.set_limit(Duration::from_millis(250));
        assert_eq!(hold(&mut limit, start + held, held).len(), 40);
        // Mid-hold too: a repeat already waiting is due at the new limit.
        limit.pressed(start);
        assert!(!limit.repeated(FORWARD, start + Duration::from_millis(100)));
        limit.set_limit(Duration::from_millis(1000));
        assert_eq!(limit.due(), Some(start + Duration::from_millis(1000)));
    }

    #[test]
    fn a_press_advances_at_once_and_a_late_repeat_too() {
        let mut limit = AdvanceLimit::new(LIMIT);
        let start = Instant::now();
        limit.pressed(start);
        // Tapping again is a press, not a repeat: it is never held back.
        limit.pressed(start + Duration::from_millis(50));
        assert!(!limit.repeated(FORWARD, start + Duration::from_millis(100)));
        assert_eq!(limit.due(), Some(start + Duration::from_millis(550)));
        assert!(limit.repeated(FORWARD, start + Duration::from_millis(600)));
        assert_eq!(limit.due(), None);
    }

    #[test]
    fn letting_go_of_the_other_key_keeps_the_waiting_repeat() {
        let mut limit = AdvanceLimit::new(LIMIT);
        let start = Instant::now();
        limit.pressed(start);
        let skip_back = Travel::Skip(Direction::Backward, 10);
        assert!(!limit.repeated(skip_back, start + Duration::from_millis(300)));
        limit.released(Direction::Forward);
        assert_eq!(
            limit.tick(start + Duration::from_millis(500)),
            Some(skip_back),
            "a held Ctrl+U keeps jumping ten"
        );
    }
}
