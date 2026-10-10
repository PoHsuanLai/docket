//! What restoring changes, and why it can be refused.

use super::WorkPath;
use serde::{Deserialize, Serialize};

/// SHA-256 over the plan's three sorted lists and the target tree id, in lower-case hex; binds a
/// confirmation to exactly what the person saw.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PlanDigest(pub String);

/// What restoring changes, from the workspace as it is now to the saved point.
///
/// - `changed`: exists now and then, content differs (written back).
/// - `added`: existed then, missing now (brought back).
/// - `removed`: exists now, did not then (deleted; the agent or the person made it since).
///
/// Ignored files are never in any list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorePlan {
    /// Files put back to their older content.
    pub changed: Vec<WorkPath>,
    /// Files brought back.
    pub added: Vec<WorkPath>,
    /// Files deleted.
    pub removed: Vec<WorkPath>,
    /// Binds a confirmation to this plan.
    pub digest: PlanDigest,
}

/// Why a restore point could not be listed, planned or restored.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CheckpointFault {
    /// No such point in the session.
    #[error("there is no such restore point")]
    NoSuchPoint,
    /// The point was cleared.
    #[error("that restore point has been cleared")]
    Gone,
    /// The session has a turn in flight.
    #[error("a turn is still running")]
    TurnRunning,
    /// The folder changed since the plan was shown.
    #[error("the folder changed since the plan was shown")]
    PlanStale,
    /// The folder cannot keep restore points.
    #[error("this folder cannot keep restore points")]
    NoHistory,
    /// Anything else.
    #[error("the restore could not be completed")]
    Failed,
}
