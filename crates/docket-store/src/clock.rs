//! The system clock: wall-clock seconds, and deadlines that need no async runtime. A deadline
//! sleeps on a short-lived thread of its own (see [`crate::thread_sleep`]) that is gone when the
//! deadline future is dropped, so it works under any executor.

use crate::thread_sleep;
use docket_vocab::{Clock, Millis};
use prov::UnixSeconds;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The one system `Clock`.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixSeconds {
        let seconds = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));
        UnixSeconds(seconds)
    }

    async fn after(&self, wait: Millis) {
        if wait.0 > 0 {
            thread_sleep(Duration::from_millis(u64::from(wait.0))).await;
        }
    }
}
