//! The restore points of a session (`Checkpoint.List` and `Checkpoint.Plan`). Restoring one is
//! not here: it is the action `checkpoints.restore`, performed like any other.

use crate::intents::{ClientError, Intents};
use crate::transport::Transport;
use docket_core::{
    CheckpointFault, CheckpointId, CheckpointList, IntentsReply, IntentsRequest, RestorePlan,
    Rewind, TurnId, Workspace,
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

    /// Opens a session that only keeps restore points for an agent the caller watches but does
    /// not host (`Checkpoint.Watch`). `label` is display text. A terminal only.
    pub async fn checkpoint_watch(
        &self,
        workspace: Workspace,
        label: String,
        rewind: Rewind,
    ) -> Result<SessionId, ClientError> {
        let request = IntentsRequest::CheckpointWatch {
            workspace,
            label,
            rewind,
        };
        self.ask(request, |r| match r {
            IntentsReply::CheckpointWatching(session) => Some(session),
            _ => None,
        })
        .await
    }

    /// The watched agent started working: takes a restore point and starts the running turn
    /// (`Checkpoint.Mark`). End the turn it answers with `session_turn_ended`.
    pub async fn checkpoint_mark(&self, session: SessionId) -> Result<TurnId, ClientError> {
        self.ask(IntentsRequest::CheckpointMark { session }, |r| match r {
            IntentsReply::TurnRecorded(turn) => Some(turn),
            _ => None,
        })
        .await
    }
}
