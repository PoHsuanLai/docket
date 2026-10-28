//! `Router::restore_session`: a session read back from its log, and the lazy restore a request
//! that names it triggers. The rules are `rebuild`'s; this is the log read and the insertion.

use crate::router::Router;
use crate::seams::Seams;
use docket_context::rebuild::{number_of, rebuild};
use docket_context::state::SessionKind;
use docket_context::wal::Writer;
use docket_session::{
    Claimant, LogFault, Logged, PageSize, PlanRefusal, Seq, SessionLog, Standing, may_restore_plan,
    resume_plan,
};
use docket_vocab::{CallerRole, Clock};
use docket_wire::{CallerId, IntentsRequest};
use porter_ids::Count;
use prov::SessionId;
use std::sync::Arc;

/// Rows read from the log at a time.
const PAGE: PageSize = PageSize(Count(200));

/// The most pages one session's log is read in, so at most `MAX_LOG_PAGES * 200` rows (about
/// 100,000). A log that goes on longer, or whose `next` never ends, is not restored:
/// [`RestoreFault::TooLong`].
pub(crate) const MAX_LOG_PAGES: usize = 512;

/// Why a session was not restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RestoreFault {
    /// The router already holds this session.
    #[error("the session is already live")]
    Live,
    /// The log could not be read; nothing was restored.
    #[error("the session log could not be read")]
    Log(LogFault),
    /// The log is longer than a restore reads (`MAX_LOG_PAGES` pages), or never ends.
    #[error("the session log is too long to restore")]
    TooLong,
    /// The log holds no session of this name, or no longer holds its opening.
    #[error("the log holds no such session")]
    Unknown(PlanRefusal),
    /// The session is stored but belongs to another caller: told to nobody, the request is
    /// answered as for an unknown session.
    #[error("the log holds no such session")]
    NotYours,
}

/// What a restore brought back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restored {
    /// Where the session stands: taking turns, paused, or closed for display.
    pub standing: Standing,
    /// The taint it holds, never lower than the log's.
    pub taint: docket_session::Taint,
    /// How many calls a crash cut off: told to the planner, not run again.
    pub interrupted: Count,
}

impl<S: Seams> Router<S> {
    /// Every row of `id`'s log, oldest first, in at most `MAX_LOG_PAGES` pages.
    pub(crate) async fn rows_of(&self, id: &SessionId) -> Result<Vec<Logged>, RestoreFault> {
        read_pages(self.seams.log(), id, MAX_LOG_PAGES).await
    }

    /// Rebuilds `id` from its log and holds it. The policy writer is not asked, the taint is the
    /// log's or higher, handles come back as labels, a call that never ended ends interrupted
    /// and is not run again, and the budget's wall clock starts now. A session whose log has a
    /// gap or an unreadable entry comes back closed, for display, tainted.
    pub async fn restore_session(&self, id: &SessionId) -> Result<Restored, RestoreFault> {
        self.restore_for(id, None).await
    }

    /// `restore_session` for a request: with a claimant, only the opener, the shell or the
    /// companion gets the session, and an editor also a helper's conversation (`may_restore_plan`); without one the daemon itself is asking.
    pub(crate) async fn restore_for(
        &self,
        id: &SessionId,
        claimant: Option<Claimant<'_>>,
    ) -> Result<Restored, RestoreFault> {
        if self.locked().sessions.contains_key(id) {
            return Err(RestoreFault::Live);
        }
        let rows = self.rows_of(id).await?;
        let plan = resume_plan(&rows).map_err(RestoreFault::Unknown)?;
        if let Some(claim) = claimant
            && !may_restore_plan(claim, &plan)
        {
            return Err(RestoreFault::NotYours);
        }
        let now = self.seams.clock().now();
        let mut rebuilt = rebuild(id, &plan, now);
        rebuilt.record.checkpoints = crate::checkpoint_turn::notes_of(&rows);
        // The last mode a turn named stands; a log that names none starts in the start mode.
        rebuilt.record.mode = self.floor.clamp_mode(
            plan.turns
                .iter()
                .rev()
                .find_map(|turn| turn.mode)
                .unwrap_or_else(|| self.agent_config().start_mode),
        );
        let next = rows.last().map_or(Seq(0), |r| r.seq.next());
        let restored = Restored {
            standing: plan.standing.clone(),
            taint: plan.taint,
            interrupted: Count(u32::try_from(plan.interrupted.len()).unwrap_or(u32::MAX)),
        };
        let mut st = self.locked();
        if st.sessions.contains_key(id) {
            return Err(RestoreFault::Live);
        }
        st.reserve(rebuilt.reserve);
        if rebuilt.record.wal.is_on() {
            st.lanes.insert(
                id.clone(),
                Arc::new(futures_util::lock::Mutex::new(Writer::at(
                    next,
                    rebuilt.durable,
                ))),
            );
        }
        // A watch session has no task.
        if matches!(rebuilt.record.kind, SessionKind::Agent)
            && st.tasks.get(&rebuilt.task.task).is_none()
        {
            st.tasks.insert(rebuilt.task);
        }
        st.sessions.insert(id.clone(), rebuilt.record);
        Ok(restored)
    }

    /// Reads which sessions the log holds and sees to it that no id minted from now on is one of
    /// them (a restart begins counting again, and a new session must not take the name of an old
    /// one). The daemon calls it once at start. The sessions come back for a listing; each is
    /// restored when a request names it (`restore_session`).
    pub async fn adopt_sessions(&self) -> Result<Vec<SessionId>, LogFault> {
        let ids = self.seams.log().sessions().await?;
        let top = ids.iter().map(|i| number_of(i.as_str())).max().unwrap_or(0);
        self.locked().reserve(top);
        Ok(ids)
    }

    /// Restores the session a request names when the router does not hold it. A session that
    /// cannot be restored, or is not the caller's to restore, stays unknown, and the request is answered as for any unknown one.
    pub(crate) async fn restore_named(
        &self,
        caller: &CallerId,
        role: CallerRole,
        request: &IntentsRequest,
    ) {
        let Some(id) = named_session(request) else {
            return;
        };
        if self.locked().sessions.contains_key(id) {
            return;
        }
        let claim = Claimant {
            role,
            app: &caller.app.name,
        };
        let _ = self.restore_for(id, Some(claim)).await;
    }
}

/// Every row of `id`'s log, oldest first, read `PAGE` rows at a time; a log that has not ended
/// after `most` pages is `TooLong` (a store whose `next` repeats would otherwise never end).
async fn read_pages<L: SessionLog>(
    log: &L,
    id: &SessionId,
    most: usize,
) -> Result<Vec<Logged>, RestoreFault> {
    let mut rows = Vec::new();
    let mut from: Option<Seq> = None;
    for _ in 0..most {
        let page = log.page(id, from, PAGE).await.map_err(RestoreFault::Log)?;
        rows.extend(page.rows);
        match page.next {
            Some(next) => from = Some(next),
            None => return Ok(rows),
        }
    }
    Err(RestoreFault::TooLong)
}

/// The session a request names, if it names one.
pub(crate) fn named_session(request: &IntentsRequest) -> Option<&SessionId> {
    use IntentsRequest as R;
    match request {
        R::Perform { session, .. } | R::DryRun { session, .. } => session.as_ref(),
        R::Context { session, .. }
        | R::SessionTurn { session, .. }
        | R::SessionClose { session }
        | R::SessionResolve { session, .. }
        | R::SessionDisplay { session, .. }
        | R::SessionDisplayLabelled { session, .. }
        | R::SessionRead { session, .. }
        | R::SessionTaskPolicy { session }
        | R::SessionWiden { session, .. }
        | R::SessionNote { session, .. }
        | R::SessionRecall { session, .. }
        | R::SessionNarrow { session, .. }
        | R::SessionHandles { session }
        | R::CheckpointList { session }
        | R::CheckpointPlan { session, .. }
        | R::CheckpointHandover { session, .. }
        | R::MessageSend { session, .. } => Some(session),
        R::SessionMode(ask) => Some(&ask.session),
        R::Manifests
        | R::IndexPush(_)
        | R::IndexReset { .. }
        | R::Search(_)
        | R::SearchCancel(_)
        | R::Preview(_)
        | R::Suggest(_)
        | R::Undo(_)
        | R::UndoAll(_)
        | R::SessionOpen(_)
        // A turn that ended names a session that is live, or has no running turn at all.
        | R::SessionTurnEnded { .. }
        | R::MessageInbox(_)
        | R::GateGrant(_)
        | R::GateCheck(_)
        | R::ControlHalt { .. }
        | R::ControlResume { .. }
        | R::ControlState
        | R::ControlJournal(_)
        | R::ControlTerminalGrants
        | R::ControlTerminalRevoke(_)
        | R::ControlStandingGrants
        | R::ControlStandingRevoke(_)
        | R::SessionStored { .. } => None,
        // A request of an unknown kind names no session, so no session is restored for it.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_session::{Appended, LogPage, SessionEntry};

    /// A log whose every page says there is another.
    struct Endless;

    impl SessionLog for Endless {
        async fn append(
            &self,
            _session: &SessionId,
            seq: Seq,
            _entry: &SessionEntry,
        ) -> Result<Appended, LogFault> {
            Ok(Appended { seq })
        }

        async fn page(
            &self,
            _session: &SessionId,
            _from: Option<Seq>,
            _size: PageSize,
        ) -> Result<LogPage, LogFault> {
            Ok(LogPage {
                rows: Vec::new(),
                next: Some(Seq(1)),
            })
        }

        async fn sessions(&self) -> Result<Vec<SessionId>, LogFault> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn a_log_that_never_ends_is_too_long_not_a_hang() {
        let id = SessionId::parse("s-1").expect("session");
        assert_eq!(
            read_pages(&Endless, &id, 3).await,
            Err(RestoreFault::TooLong)
        );
    }
}
