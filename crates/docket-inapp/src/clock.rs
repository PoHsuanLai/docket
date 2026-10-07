//! A real clock for the router that needs no async runtime. `now` reads the system time;
//! `after` is a future that a single timer thread wakes, so the reviewer race works under any
//! executor (tokio, async-std, smol, a UI toolkit's) with no dependency at all: std threads and a
//! `Condvar` are the simplest permissive answer, and a crate (`futures-timer`, `async-io`) would
//! add a runtime-flavoured dependency for what is a few dozen lines.
//!
//! The timer thread starts at the first deadline and ends within a second of the last clock being
//! dropped (it holds the queue weakly, and the queue holds only flags, so a dropped `after`
//! future costs nothing but the raise of a flag nobody reads). The queue is pure ([`Timers`]):
//! deadlines in, the due flags out, tested with instants the test makes up.

use docket_core::Millis;
use docket_router::{Clock, Flag};
use prov::UnixSeconds;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// The pending deadlines, each with the flag to raise. Pure: it never reads a clock or sleeps;
/// the caller says what time it is. Equal deadlines come due in the order they were added.
#[derive(Debug, Default)]
pub(crate) struct Timers {
    waiting: BTreeMap<(Instant, u64), Arc<Flag>>,
    issued: u64,
}

impl Timers {
    /// Queues `flag` to be raised at `at`.
    pub(crate) fn add(&mut self, at: Instant, flag: Arc<Flag>) {
        self.issued += 1;
        self.waiting.insert((at, self.issued), flag);
    }

    /// Takes every flag due at `now`, earliest first.
    pub(crate) fn due(&mut self, now: Instant) -> Vec<Arc<Flag>> {
        let later = self.waiting.split_off(&(now, u64::MAX));
        let due = std::mem::replace(&mut self.waiting, later);
        due.into_values().collect()
    }

    /// The earliest deadline still waiting.
    pub(crate) fn next(&self) -> Option<Instant> {
        self.waiting.keys().next().map(|(at, _)| *at)
    }
}

#[derive(Debug, Default)]
struct Shared {
    timers: Mutex<Timers>,
    changed: Condvar,
}

impl Shared {
    fn locked(&self) -> MutexGuard<'_, Timers> {
        self.timers.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// How long the thread sleeps when nothing waits.
const IDLE: Duration = Duration::from_secs(1);

/// The timer thread: raises what is due, then waits for the next deadline or a new one.
fn run(shared: &Weak<Shared>) {
    while let Some(shared) = shared.upgrade() {
        let mut timers = shared.locked();
        let due = timers.due(Instant::now());
        if due.is_empty() {
            let wait = timers
                .next()
                .map_or(IDLE, |at| at.saturating_duration_since(Instant::now()));
            let _ = shared.changed.wait_timeout(timers, wait);
        } else {
            drop(timers);
            due.iter().for_each(|flag| flag.raise());
        }
    }
}

/// Whether the timer thread has been started.
#[derive(Debug, Default)]
enum Timer {
    #[default]
    NotStarted,
    Running,
}

/// The system clock: wall-clock seconds, and deadlines woken by one background thread.
#[derive(Debug, Clone, Default)]
pub struct SystemClock {
    shared: Arc<Shared>,
    timer: Arc<Mutex<Timer>>,
}

impl SystemClock {
    /// A clock. No thread runs until the first `after`.
    pub fn new() -> Self {
        Self::default()
    }

    fn ensure_thread(&self) {
        let mut timer = self.timer.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(*timer, Timer::NotStarted) {
            let weak = Arc::downgrade(&self.shared);
            // A thread that cannot be made leaves the deadline unraised: the reviewer race then
            // waits on the reviewer alone, which can only mean the person is asked sooner or
            // later, never that anything is allowed.
            if std::thread::Builder::new()
                .name("docket-timer".to_owned())
                .spawn(move || run(&weak))
                .is_ok()
            {
                *timer = Timer::Running;
            }
        }
    }
}

impl Clock for SystemClock {
    fn now(&self) -> UnixSeconds {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        UnixSeconds(seconds)
    }

    fn after(&self, wait: Millis) -> impl Future<Output = ()> + Send {
        let flag = Flag::new();
        match wait {
            Millis(0) => flag.raise(),
            Millis(ms) => {
                self.ensure_thread();
                let at = Instant::now() + Duration::from_millis(u64::from(ms));
                self.shared.locked().add(at, flag.clone());
                self.shared.changed.notify_one();
            }
        }
        async move { flag.up().await }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};

    #[test]
    fn deadlines_come_due_in_order_and_only_when_they_are_due() {
        let t0 = Instant::now();
        let at = |ms| t0 + Duration::from_millis(ms);
        let flags: Vec<_> = (0..3).map(|_| Flag::new()).collect();
        let mut timers = Timers::default();
        timers.add(at(30), flags[2].clone());
        timers.add(at(10), flags[0].clone());
        timers.add(at(10), flags[1].clone());
        assert_eq!(timers.next(), Some(at(10)));
        assert!(timers.due(at(9)).is_empty());
        let first = timers.due(at(10));
        assert_eq!(first.len(), 2);
        assert!(
            Arc::ptr_eq(&first[0], &flags[0]),
            "equal deadlines wake in order"
        );
        assert!(Arc::ptr_eq(&first[1], &flags[1]));
        assert_eq!(timers.next(), Some(at(30)));
        assert_eq!(timers.due(at(31)).len(), 1);
        assert_eq!(timers.next(), None);
    }

    #[test]
    fn no_time_at_all_is_already_done_and_starts_no_thread() {
        let clock = SystemClock::new();
        let mut waiting = Box::pin(clock.after(Millis(0)));
        let mut cx = Context::from_waker(Waker::noop());
        assert_eq!(Pin::new(&mut waiting).poll(&mut cx), Poll::Ready(()));
        assert!(matches!(
            *clock.timer.lock().expect("lock"),
            Timer::NotStarted
        ));
    }
}
