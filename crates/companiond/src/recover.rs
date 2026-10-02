//! Restart. companiond keeps no state of its own: the roster and the front task are rebuilt from
//! the records the eventlog holds (its own session records, the router's task and message
//! events, the computer-use runs, the episodes).

use agent_loop::{Rebuilt, ReplayEvent, rebuild};
use std::future::Future;

/// Why the stored records could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ReplayFault {
    /// The router or memoryd is not there.
    #[error("the record is not available")]
    Unavailable,
    /// A record is not in the form its owner writes.
    #[error("a stored record is malformed")]
    Malformed,
}

/// Where the stored events come from.
pub trait ReplaySource: Send + Sync {
    /// The events since the retention window opened, oldest first.
    fn events(&self) -> impl Future<Output = Result<Vec<ReplayEvent>, ReplayFault>> + Send;
}

/// Rebuilds the roster and the front task from `source`.
pub async fn recover<S: ReplaySource>(source: &S) -> Result<Rebuilt, ReplayFault> {
    Ok(rebuild(&source.events().await?))
}
