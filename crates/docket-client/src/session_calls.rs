//! The companion's own members of `Intents1.Session`: what it reads from memory through the
//! router, the reader it asks, the episode it hands over, and the task policy it holds. They sit
//! apart from the rest of the caller side because only the companion's role may make them.

use crate::intents::{ClientError, Intents};
use crate::transport::Transport;
use docket_core::{
    HandleCard, IntentsReply, IntentsRequest, NoteAsk, ReadAsk, RecallAsk, RecallView, Reveal,
    TaskPolicy, UserTurn, Value,
};
use prov::SessionId;

impl<T: Transport> Intents<T> {
    /// What memory knows for the session's Space, every untrusted text a handle
    /// (`Session.Recall`, the companion only).
    pub async fn session_recall(
        &self,
        session: SessionId,
        ask: RecallAsk,
    ) -> Result<RecallView, ClientError> {
        self.ask(
            IntentsRequest::SessionRecall { session, ask },
            |r| match r {
                IntentsReply::Recalled(view) => Some(view),
                _ => None,
            },
        )
        .await
    }

    /// Asks the quarantined reader (`Session.Read`, the companion only): a closed-set value
    /// comes back plain, any text in it as a handle.
    pub async fn session_read(
        &self,
        session: SessionId,
        ask: ReadAsk,
    ) -> Result<Reveal<Value>, ClientError> {
        self.ask(IntentsRequest::SessionRead { session, ask }, |r| match r {
            IntentsReply::Read(answer) => Some(answer),
            _ => None,
        })
        .await
    }

    /// Hands the router an episode to record (`Session.Note`, the companion only): the idle
    /// pass's narrative, or a side conversation's episode.
    pub async fn session_note(&self, session: SessionId, note: NoteAsk) -> Result<(), ClientError> {
        self.ask(IntentsRequest::SessionNote { session, note }, |r| match r {
            IntentsReply::Done => Some(()),
            _ => None,
        })
        .await
    }

    /// The session's task policy, if the writer made one (`Session.TaskPolicy`).
    pub async fn session_task_policy(
        &self,
        session: SessionId,
    ) -> Result<Option<TaskPolicy>, ClientError> {
        self.ask(IntentsRequest::SessionTaskPolicy { session }, |r| match r {
            IntentsReply::TaskPolicy(policy) => Some(policy.map(|p| *p)),
            _ => None,
        })
        .await
    }

    /// Narrows a subagent's task policy from what the person said to it (`Session.Narrow`, the
    /// companion only): never wider than the policy it has.
    pub async fn session_narrow(
        &self,
        session: SessionId,
        turn: UserTurn,
    ) -> Result<(), ClientError> {
        self.ask(
            IntentsRequest::SessionNarrow { session, turn },
            |r| match r {
                IntentsReply::Done => Some(()),
                _ => None,
            },
        )
        .await
    }

    /// What the session holds by handle: shape, source and size, never the content
    /// (`Session.Handles`, the companion only).
    pub async fn session_handles(
        &self,
        session: SessionId,
    ) -> Result<Vec<HandleCard>, ClientError> {
        self.ask(IntentsRequest::SessionHandles { session }, |r| match r {
            IntentsReply::Handles(cards) => Some(cards),
            _ => None,
        })
        .await
    }
}
