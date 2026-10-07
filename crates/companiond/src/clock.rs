//! The companion's clock: the system's, or one a test moves by hand. companiond is one of the
//! few crates that may read the system clock (CONVENTIONS 10); everything below it is handed a
//! time.

use prov::UnixSeconds;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

/// What tells the time.
#[derive(Debug, Clone)]
pub enum Clock {
    /// The system's.
    System,
    /// A test's: it says what it was last set to.
    Manual(Arc<AtomicI64>),
}

impl Clock {
    /// A clock that reads `at` until it is set again; the handle moves it.
    pub fn manual(at: UnixSeconds) -> (Self, Arc<AtomicI64>) {
        let hand = Arc::new(AtomicI64::new(at.0));
        (Clock::Manual(hand.clone()), hand)
    }

    /// Now.
    pub fn now(&self) -> UnixSeconds {
        match self {
            Clock::System => UnixSeconds(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX)),
            ),
            Clock::Manual(hand) => UnixSeconds(hand.load(Ordering::SeqCst)),
        }
    }
}

impl docket_tasks::Now for Clock {
    fn now(&self) -> UnixSeconds {
        Clock::now(self)
    }
}
