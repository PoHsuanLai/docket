//! The seam: every file and history effect of a restore point.

use crate::ids::{Saved, WorkRoot};
use docket_core::{CheckpointId, PlanDigest, RestorePlan};
use porter_core::{Count, UnixSeconds};
use prov::SessionId;
use std::future::Future;

/// Why a store could not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum StoreFault {
    /// The folder has no history to keep points in.
    #[error("no history to keep points in")]
    NoHistory,
    /// The folder holds more files than the limit.
    #[error("too many files")]
    TooLarge,
    /// The session holds no such point.
    #[error("no such point")]
    NoSuchPoint,
    /// The folder is not what the plan was made from.
    #[error("the folder changed under the plan")]
    PlanStale,
    /// Anything else, including a point number that is already used.
    #[error("the store failed")]
    Failed,
}

/// Save the folder as point `id`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TakeAsk {
    /// The folder.
    pub root: WorkRoot,
    /// The session the point belongs to.
    pub session: SessionId,
    /// The number to save it under; a number already used is refused (`Failed`), never replaced.
    pub id: CheckpointId,
    /// The time to record.
    pub at: UnixSeconds,
    /// A folder with more files than this is `TooLarge`.
    pub max: Count,
}

/// What restoring point `id` would change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanAsk {
    /// The folder.
    pub root: WorkRoot,
    /// The session.
    pub session: SessionId,
    /// The point.
    pub id: CheckpointId,
}

/// Restore point `id`, if the folder is still what the plan was made from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyAsk {
    /// The folder.
    pub root: WorkRoot,
    /// The session.
    pub session: SessionId,
    /// The point.
    pub id: CheckpointId,
    /// The digest of the plan the person confirmed.
    pub expect: PlanDigest,
}

/// Forget points.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DropAsk {
    /// The folder.
    pub root: WorkRoot,
    /// The session.
    pub session: SessionId,
    /// The points to forget; one the store does not hold is skipped.
    pub ids: Vec<CheckpointId>,
}

/// All file and history effects. Methods never touch the person's own history (index, branches,
/// stash) or, except `apply`, the files of the folder.
pub trait CheckpointStore: Send + Sync {
    /// Saves the folder as a new point.
    fn take(&self, ask: TakeAsk) -> impl Future<Output = Result<Saved, StoreFault>> + Send;

    /// The points held for `session`, oldest first.
    fn held(
        &self,
        root: &WorkRoot,
        session: &SessionId,
    ) -> impl Future<Output = Result<Vec<Saved>, StoreFault>> + Send;

    /// What restoring a point would change, from the folder as it is now. Writes nothing.
    fn plan(&self, ask: PlanAsk) -> impl Future<Output = Result<RestorePlan, StoreFault>> + Send;

    /// Re-derives the plan, refuses `PlanStale` if its digest is not `expect`, then writes
    /// `changed` and `added` from the point and deletes `removed`. Returns what it did.
    fn apply(&self, ask: ApplyAsk) -> impl Future<Output = Result<RestorePlan, StoreFault>> + Send;

    /// Forgets points; how many were forgotten.
    fn drop_points(&self, ask: DropAsk) -> impl Future<Output = Result<Count, StoreFault>> + Send;
}

/// A store for a build that cannot keep history: every ask is `NoHistory`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoStore;

impl CheckpointStore for NoStore {
    async fn take(&self, _ask: TakeAsk) -> Result<Saved, StoreFault> {
        Err(StoreFault::NoHistory)
    }

    async fn held(&self, _root: &WorkRoot, _session: &SessionId) -> Result<Vec<Saved>, StoreFault> {
        Err(StoreFault::NoHistory)
    }

    async fn plan(&self, _ask: PlanAsk) -> Result<RestorePlan, StoreFault> {
        Err(StoreFault::NoHistory)
    }

    async fn apply(&self, _ask: ApplyAsk) -> Result<RestorePlan, StoreFault> {
        Err(StoreFault::NoHistory)
    }

    async fn drop_points(&self, _ask: DropAsk) -> Result<Count, StoreFault> {
        Err(StoreFault::NoHistory)
    }
}
