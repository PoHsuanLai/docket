//! The turn an agent is working on now. It starts when `Session.Turn` has recorded the turn and
//! saved its restore point, and ends when the host says so (`Session.TurnEnded`, for that turn
//! only), when the session closes, or when it has run longer than `agent.checkpoints.turn_max_s`
//! (a host that died never says). While one runs, a restore is refused and the list says so.
//!
//! Why a host's word is enough: `Session.TurnEnded` can only mark a turn ended. It never starts,
//! allows or skips anything. A wrong word from a host (or one forged by another caller of the
//! host's role) lets a restore the person already confirmed on the sheet go ahead earlier than it
//! would; that restore still needs the person's hold-to-confirm, is bound to the plan they saw,
//! and keeps a safety point. And only the session's opener may end its turns (as for
//! `Session.Turn`).

use crate::router::Router;
use crate::seams::{Clock, Seams};
use docket_core::{CallerId, CallerRole, IntentsReply, TurnId, TurnState, WireRefusal};
use prov::{SessionId, UnixSeconds};

/// A turn that has begun and not ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RunningTurn {
    /// Which turn.
    pub(crate) turn: TurnId,
    /// When it began, by the router's clock.
    pub(crate) since: UnixSeconds,
}

impl<S: Seams> Router<S> {
    /// Marks `turn` as running in `session`, in place of any turn that never ended.
    pub(crate) fn begin_turn(&self, session: &SessionId, turn: TurnId) {
        let since = self.seams.clock().now();
        self.locked()
            .running
            .insert(session.clone(), RunningTurn { turn, since });
    }

    /// Whether the agent of `session` is working now. A turn that has run longer than the
    /// configured limit counts as ended, and is forgotten.
    pub(crate) fn turn_state(&self, session: &SessionId) -> TurnState {
        let limit = i64::from(self.agent_config().checkpoint_turn_max.0);
        let now = self.seams.clock().now();
        let mut st = self.locked();
        let expired = st
            .running
            .get(session)
            .map(|run| now.0.saturating_sub(run.since.0) > limit);
        match expired {
            None => TurnState::Idle,
            Some(true) => {
                st.running.remove(session);
                TurnState::Idle
            }
            Some(false) => TurnState::Running,
        }
    }

    /// `.Session.TurnEnded`: forgets the running turn if it is `turn`; any other turn is
    /// ignored (a late word about an earlier turn must not end the one that runs now).
    pub(crate) fn session_turn_ended(
        &self,
        caller: &CallerId,
        role: CallerRole,
        session: &SessionId,
        turn: TurnId,
    ) -> IntentsReply {
        let mut st = self.locked();
        let Some(record) = st.sessions.get(session) else {
            return IntentsReply::Refused(WireRefusal::NoSuchSession);
        };
        if matches!(
            role,
            CallerRole::Field | CallerRole::Editor | CallerRole::AcpAgent
        ) && record.opener != caller.app.name
        {
            return IntentsReply::Refused(WireRefusal::NotAllowed);
        }
        if st.running.get(session).is_some_and(|run| run.turn == turn) {
            st.running.remove(session);
        }
        IntentsReply::Done
    }
}
