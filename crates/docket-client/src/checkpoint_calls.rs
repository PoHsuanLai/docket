//! The restore points of a session (`Checkpoint.List` and `Checkpoint.Plan`). Restoring one is
//! not here: it is the action `checkpoints.restore`, performed like any other.

use crate::intents::{ClientError, Intents};
use crate::transport::Transport;
use docket_core::{
    CheckpointFault, CheckpointId, CheckpointList, IntentsReply, IntentsRequest, RestorePlan,
};
use prov::SessionId;

impl<T: Transport> Intents<T> {
    /// The session's restore points, oldest first (`Checkpoint.List`).
    pub async fn checkpoint_list(&self, session: SessionId) -> Result<CheckpointList, ClientError> {
        self.ask(IntentsRequest::CheckpointList { session }, |r| match r {
            IntentsReply::Checkpoints(list) => Some(*list),
            _ => None,
        })
        .await
    }

    /// What restoring `id` would change, or why that cannot be said (`Checkpoint.Plan`).
    /// Read-only.
    pub async fn checkpoint_plan(
        &self,
        session: SessionId,
        id: CheckpointId,
    ) -> Result<Result<RestorePlan, CheckpointFault>, ClientError> {
        self.ask(
            IntentsRequest::CheckpointPlan { session, id },
            |r| match r {
                IntentsReply::CheckpointPlan(plan) => Some(plan),
                _ => None,
            },
        )
        .await
    }
}
