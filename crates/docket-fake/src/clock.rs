//! A virtual clock: it says the same instant until a test moves it, and the router's timers
//! (`Clock::after`) complete only when the test has advanced it far enough. No wall time passes.

use docket_core::{Millis, Seconds};
use docket_router::{Clock, Flag, Queue};
use prov::UnixSeconds;
use std::sync::{Arc, Mutex};

#[derive(Debug, Default)]
struct Sky {
    /// Virtual milliseconds since the clock was made.
    elapsed: u64,
    /// The timers waiting: when each is due, and its flag.
    timers: Vec<(u64, Arc<Flag>)>,
    /// Every wait the router asked for, in order.
    asked: Vec<Millis>,
    /// Seconds `pass` has added to the instant `now` reports.
    passed: i64,
}

/// A clock that says the same instant until `advance` moves it. `after(Millis(0))` completes at
/// once; any other wait completes when the test has advanced the clock by that much.
#[derive(Debug, Clone)]
pub struct FixedClock {
    start: UnixSeconds,
    sky: Arc<Mutex<Sky>>,
    arrivals: Arc<Queue<Millis>>,
}

impl FixedClock {
    /// A clock at `start`.
    pub fn at(start: UnixSeconds) -> Self {
        Self {
            start,
            sky: Arc::default(),
            arrivals: Queue::new(),
        }
    }

    fn sky(&self) -> std::sync::MutexGuard<'_, Sky> {
        self.sky.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Moves virtual time forward and completes every timer that is now due.
    pub fn advance(&self, by: Millis) {
        let due: Vec<Arc<Flag>> = {
            let mut sky = self.sky();
            sky.elapsed += u64::from(by.0);
            let now = sky.elapsed;
            let (due, waiting) = std::mem::take(&mut sky.timers)
                .into_iter()
                .partition::<Vec<_>, _>(|(at, _)| *at <= now);
            sky.timers = waiting;
            due.into_iter().map(|(_, flag)| flag).collect()
        };
        due.iter().for_each(|flag| flag.raise());
    }

    /// Moves the instant `now` reports forward by `by`. Timers are not touched (`advance`
    /// moves those): this is for a rule that reads how old something is.
    pub fn pass(&self, by: Seconds) {
        let mut sky = self.sky();
        sky.passed = sky.passed.saturating_add(i64::from(by.0));
    }

    /// Every wait the router has asked for so far, in order.
    pub fn asked(&self) -> Vec<Millis> {
        self.sky().asked.clone()
    }

    /// Resolves with the next wait the router asks for that has not been read yet: a test waits
    /// on this instead of on time.
    pub async fn next_ask(&self) -> Millis {
        self.arrivals.next().await
    }
}

impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(self.start.0.saturating_add(self.sky().passed))
    }

    async fn after(&self, wait: Millis) {
        let flag = {
            let mut sky = self.sky();
            sky.asked.push(wait);
            if wait.0 == 0 {
                None
            } else {
                let flag = Flag::new();
                let due = sky.elapsed + u64::from(wait.0);
                sky.timers.push((due, flag.clone()));
                Some(flag)
            }
        };
        self.arrivals.push(wait);
        if let Some(flag) = flag {
            flag.up().await;
        }
    }
}
