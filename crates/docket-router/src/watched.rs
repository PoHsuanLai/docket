//! Watch sessions: a terminal sees a CLI agent in one of its panes that docket does not host,
//! and wants the agent's file changes to be undoable. `Checkpoint.Watch` opens a session that
//! holds restore points and nothing else (its log is a watch opening and `Checkpoint` notes),
//! and `Checkpoint.Mark` says the agent started working: a point is saved as at a turn start
//! (or `Skipped(AgentKeepsOwn)` when the agent keeps its own history) and the running turn
//! begins. The terminal ends it with `Session.TurnEnded`, and closes the session with
//! `Session.Close`.
//!
//! It is not a way to run anything. The session has no task, planner or policy, and `watch_gate`
//! is the one place that refuses every request that would run something on it (a turn, a call,
//! a message, a read of its handles) before the request is answered. The person's words are not
//! recorded: `Session.Turn` records them verbatim and trusted, so it is never used for an agent
//! the router only watches.
//!
//! Only the app that opened a watch session may mark it, end its turns or close it, whatever its
//! role. A terminal may close no other kind of session.

use crate::checkpoint_restore::{CHECKPOINTS_APP, CHECKPOINTS_RESTORE};
use crate::restore::named_session;
use crate::router::Router;
use crate::seams::{Clock, Seams};
use crate::session::SessionState;
use crate::state::{SessionKind, SessionRecord};
use crate::wal::Wal;
use docket_core::{
    CallRequest, CallerId, CallerRole, IntentsReply, IntentsRequest, Rewind, TurnId, WireRefusal,
    Workspace,
};
use docket_session::{BackendKind, Opening, SessionEntry, Taint as Written};
use prov::{Actor, AgentLabel, AgentRef, SessionId, SpaceId, TaskId};

/// The longest label kept, in characters.
const LABEL_MAX: usize = 80;

/// The label as display text: no control characters, cut short.
fn label_text(label: &str) -> AgentLabel {
    AgentLabel(
        label
            .chars()
            .filter(|c| !c.is_control())
            .take(LABEL_MAX)
            .collect(),
    )
}

/// Whether `call` is the restore of a session's files, the one action a watch session allows.
fn is_restore(call: &CallRequest) -> bool {
    call.action.app.as_str() == CHECKPOINTS_APP && call.action.name.as_str() == CHECKPOINTS_RESTORE
}

/// The session a request names, including the two members that `restore_named` leaves alone.
fn session_named(request: &IntentsRequest) -> Option<&SessionId> {
    match request {
        IntentsRequest::SessionTurnEnded { session, .. }
        | IntentsRequest::CheckpointMark { session } => Some(session),
        other => named_session(other),
    }
}

/// Whether a request that names a watch session may go on. Reading its points and restoring
/// them is for any surface that may bring the session back (checked where it is answered);
/// marking, ending a turn and closing are the opener's alone.
fn watch_allows(request: &IntentsRequest, record: &SessionRecord, caller: &CallerId) -> bool {
    let opener = record.opener == caller.app.name;
    match request {
        IntentsRequest::CheckpointList { .. } | IntentsRequest::CheckpointPlan { .. } => true,
        IntentsRequest::CheckpointMark { .. }
        | IntentsRequest::SessionTurnEnded { .. }
        | IntentsRequest::SessionClose { .. } => opener,
        IntentsRequest::Perform { call, .. } | IntentsRequest::DryRun { call, .. } => {
            is_restore(call)
        }
        _ => false,
    }
}

impl<S: Seams> Router<S> {
    /// Refuses a request that would use a watch session as anything but a holder of restore
    /// points, or that a caller who did not open it makes of it. Also: a terminal closes no
    /// session but a watch session it opened.
    pub(crate) fn watch_gate(
        &self,
        caller: &CallerId,
        role: CallerRole,
        request: &IntentsRequest,
    ) -> Result<(), WireRefusal> {
        let Some(session) = session_named(request) else {
            return Ok(());
        };
        let st = self.locked();
        let watched = st
            .sessions
            .get(session)
            .filter(|record| matches!(record.kind, SessionKind::Watch(_)));
        match watched {
            Some(record) if watch_allows(request, record, caller) => Ok(()),
            Some(_) => Err(WireRefusal::NotAllowed),
            None if role == CallerRole::Cli
                && matches!(request, IntentsRequest::SessionClose { .. }) =>
            {
                Err(WireRefusal::NotAllowed)
            }
            None => Ok(()),
        }
    }

    /// `.Checkpoint.Watch`: opens a session for restore points only, in `workspace`, opened by
    /// the caller. It has a task id (the opening names one) but no task record, no policy and
    /// no space of its own (it sits in the desktop space, as an implicit session does).
    pub(crate) fn checkpoint_watch(
        &self,
        caller: &CallerId,
        workspace: Workspace,
        label: &str,
        rewind: Rewind,
    ) -> IntentsReply {
        let now = self.seams.clock().now();
        let mut st = self.locked();
        let n = st.mint();
        let (Ok(session), Ok(task)) = (
            SessionId::parse(&format!("s-{n}")),
            TaskId::parse(&format!("t-{n}")),
        ) else {
            return IntentsReply::Refused(WireRefusal::Malformed);
        };
        let space = SpaceId::desktop();
        let opener = caller.app.name.clone();
        let mut record =
            SessionRecord::new(task.clone(), Actor::Cli, opener.clone(), space.clone(), now);
        record.kind = SessionKind::Watch(rewind);
        record.cwd = Some(workspace.clone());
        record.wal = Wal::on(Written::Clean);
        record.wal.note(SessionEntry::Opened(Opening {
            task,
            space,
            opener: Some(opener),
            agent: Some(AgentRef::User),
            backend: BackendKind::Watch(rewind),
            parent: None,
            forked_from: None,
            cwd: Some(workspace),
            started_from: None,
            label: Some(label_text(label)),
        }));
        st.sessions.insert(session.clone(), record);
        IntentsReply::CheckpointWatching(session)
    }

    /// The number of a new turn of a watch session, or why it has none. The number comes from
    /// the same counter as the person's turns (`record_turn`), so it is never used twice.
    fn mark_turn(&self, session: &SessionId) -> Result<TurnId, WireRefusal> {
        let mut st = self.locked();
        let standing = st
            .sessions
            .get(session)
            .map(|record| (record.kind, record.state));
        match standing {
            None => Err(WireRefusal::NoSuchSession),
            Some((SessionKind::Watch(_), SessionState::Open(_))) => {
                Ok(TurnId(u64::from(st.mint())))
            }
            Some(_) => Err(WireRefusal::NotAllowed),
        }
    }

    /// `.Checkpoint.Mark`: the watched agent started working. What `Session.Turn` does about a
    /// restore point (decide by the watch's rewind, take, prune) and the running turn, with no
    /// turn of the person's recorded. The opener-only rule is `watch_gate`'s.
    pub(crate) async fn checkpoint_mark(&self, session: &SessionId) -> IntentsReply {
        match self.mark_turn(session) {
            Ok(turn) => {
                self.checkpoint_step(session, turn).await;
                self.begin_turn(session, turn);
                IntentsReply::Done
            }
            Err(why) => IntentsReply::Refused(why),
        }
    }
}
