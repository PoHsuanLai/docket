//! The restore points of a session (`Checkpoint.List` and `Checkpoint.Plan`). Restoring one is
//! not here: it is the action `checkpoints.restore`, performed like any other.

use crate::intents::{ClientError, Intents};
use crate::transport::Transport;
use docket_core::{
    CheckpointFault, CheckpointId, CheckpointList, HandTo, IntentsReply, IntentsRequest,
    RestorePlan, Rewind, TurnId, WatchedAgent, WatchedLine, Watching, Workspace,
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

    /// `checkpoint_watch` for a terminal that also names the agent app it watches and the app's
    /// own id for the conversation, so the shell can offer to bring it into docket
    /// (`Checkpoint.WatchAgent`). Answers the watch session and who closes it: the companion's
    /// own session when it handed this conversation over, else a new one. A terminal only.
    pub async fn checkpoint_watch_agent(
        &self,
        workspace: Workspace,
        label: String,
        rewind: Rewind,
        agent: WatchedAgent,
    ) -> Result<Watching, ClientError> {
        let request = IntentsRequest::CheckpointWatchAgent {
            workspace,
            label,
            rewind,
            agent,
        };
        self.ask(request, |r| match r {
            IntentsReply::CheckpointWatchingAgent(watching) => Some(watching),
            _ => None,
        })
        .await
    }

    /// Moves a session's conversation to the app's own terminal program or back
    /// (`Checkpoint.Handover`). `agent` names the app and its own id for the conversation when it
    /// goes to the terminal. Answers what the router holds for it. The host of the agent apps only.
    pub async fn checkpoint_handover(
        &self,
        session: SessionId,
        to: HandTo,
        agent: Option<WatchedAgent>,
    ) -> Result<Option<WatchedAgent>, ClientError> {
        self.ask(
            IntentsRequest::CheckpointHandover { session, to, agent },
            |r| match r {
                IntentsReply::HandedOver(agent) => Some(agent),
                _ => None,
            },
        )
        .await
    }

    /// The open watch sessions whose agent app a terminal named (`Checkpoint.Watched`). The
    /// companion only.
    pub async fn checkpoint_watched(&self) -> Result<Vec<WatchedLine>, ClientError> {
        self.ask(IntentsRequest::CheckpointWatched, |r| match r {
            IntentsReply::Watched(lines) => Some(lines),
            _ => None,
        })
        .await
    }
}
