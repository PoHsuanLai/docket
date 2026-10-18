//! Calls that outlast what the bus lets one call wait: started and answered with an id, then
//! asked about again until they have ended. The reply of a call is given once.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::sync::watch;

/// How long one `wait` holds a call before telling the caller to ask again: well under what the
/// bus lets a call wait for its reply.
pub const WAIT: Duration = Duration::from_secs(60);

/// The calls started and not yet collected; the oldest are forgotten beyond this.
const KEPT: usize = 8;

/// The call asked about was never started, or was collected already.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no such turn")]
pub struct NoSuchTurn;

/// The calls of one kind that are running or waiting to be collected.
pub struct Calls<T> {
    running: Mutex<BTreeMap<u64, watch::Receiver<Option<T>>>>,
    next: AtomicU64,
}

impl<T> std::fmt::Debug for Calls<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Calls")
    }
}

impl<T> Default for Calls<T> {
    fn default() -> Self {
        Self {
            running: Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(0),
        }
    }
}

impl<T: Clone + Send + Sync + 'static> Calls<T> {
    /// Starts `work` and answers the call's id; [`Self::wait`] collects its reply.
    pub fn begin(&self, work: impl Future<Output = T> + Send + 'static) -> String {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (done, reply) = watch::channel(None);
        if let Ok(mut running) = self.running.lock() {
            running.insert(id, reply);
            while running.len() > KEPT {
                running.pop_first();
            }
        }
        tokio::spawn(async move {
            let reply = work.await;
            // The caller stopped asking after it; nothing to tell.
            let _ = done.send(Some(reply));
        });
        id.to_string()
    }

    /// The reply of the call `id` once it has ended, waiting up to [`WAIT`] for it: `None` while
    /// it is still working. `lost` is the reply of a call whose task ended without one.
    pub async fn wait(&self, id: &str, lost: T) -> Result<Option<T>, NoSuchTurn> {
        let id: u64 = id.parse().map_err(|_| NoSuchTurn)?;
        let mut reply = self
            .running
            .lock()
            .ok()
            .and_then(|running| running.get(&id).cloned())
            .ok_or(NoSuchTurn)?;
        let ended = tokio::time::timeout(WAIT, reply.wait_for(Option::is_some)).await;
        let collected = match ended {
            Ok(Ok(value)) => value.clone(),
            Ok(Err(_)) => Some(lost),
            Err(_) => None,
        };
        if collected.is_some()
            && let Ok(mut running) = self.running.lock()
        {
            running.remove(&id);
        }
        Ok(collected)
    }

    /// Whether every reply has been collected.
    pub fn is_empty(&self) -> bool {
        self.running
            .lock()
            .map_or(true, |running| running.is_empty())
    }
}
