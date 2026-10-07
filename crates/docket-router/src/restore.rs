//! `Router::restore_session`: a session read back from its log, and the lazy restore a request
//! that names it triggers. The rules are `rebuild`'s; this is the log read and the insertion.

use crate::rebuild::{number_of, rebuild};
use crate::restore_rule::{Claimant, may_restore};
use crate::router::Router;
use crate::seams::{Clock, Seams};
use crate::wal::Writer;
use docket_core::{CallerId, CallerRole, IntentsRequest};
use docket_session::{
    LogFault, Logged, PageSize, PlanRefusal, Seq, SessionLog, Standing, resume_plan,
};
use porter_core::Count;
use prov::SessionId;
use std::sync::Arc;

/// Rows read from the log at a time.
const PAGE: PageSize = PageSize(Count(200));

/// Why a session was not restored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RestoreFault {
    /// The router already holds this session.
    #[error("the session is already live")]
    Live,
    /// The log could not be read; nothing was restored.
    #[error("the session log could not be read")]
    Log(LogFault),
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
    pub taint: crate::session::Taint,
    /// How many calls a crash cut off: told to the planner, not run again.
    pub interrupted: Count,
}

impl<S: Seams> Router<S> {
    /// Every row of `id`'s log, oldest first.
    async fn rows_of(&self, id: &SessionId) -> Result<Vec<Logged>, LogFault> {
        let mut rows = Vec::new();
        let mut from: Option<Seq> = None;
        loop {
            let page = self.seams.log().page(id, from, PAGE).await?;
            rows.extend(page.rows);
            match page.next {
                Some(next) => from = Some(next),
                None => return Ok(rows),
            }
        }
    }

    /// Rebuilds `id` from its log and holds it. The policy writer is not asked, the taint is the
    /// log's or higher, handles come back as labels, a call that never ended ends interrupted
    /// and is not run again, and the budget's wall clock starts now. A session whose log has a
    /// gap or an unreadable entry comes back closed, for display, tainted.
    pub async fn restore_session(&self, id: &SessionId) -> Result<Restored, RestoreFault> {
        self.restore_for(id, None).await
    }

    /// `restore_session` for a request: with a claimant, only the opener, the shell or the
    /// companion gets the session (`may_restore`); without one the daemon itself is asking.
    async fn restore_for(
        &self,
        id: &SessionId,
        claimant: Option<Claimant<'_>>,
    ) -> Result<Restored, RestoreFault> {
        if self.locked().sessions.contains_key(id) {
            return Err(RestoreFault::Live);
        }
        let rows = self.rows_of(id).await.map_err(RestoreFault::Log)?;
        let plan = resume_plan(&rows).map_err(RestoreFault::Unknown)?;
        if let Some(claim) = claimant
            && !may_restore(claim, plan.opening.opener.as_ref())
        {
            return Err(RestoreFault::NotYours);
        }
        let now = self.seams.clock().now();
        let rebuilt = rebuild(id, &plan, now);
        let next = rows.last().map_or(Seq(0), |r| r.seq.next());
        let restored = Restored {
            standing: plan.standing.clone(),
            taint: plan.taint.into(),
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
        if st.tasks.get(&rebuilt.task.task).is_none() {
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

/// The session a request names, if it names one.
fn named_session(request: &IntentsRequest) -> Option<&SessionId> {
    use IntentsRequest as R;
    match request {
        R::Perform { session, .. } | R::DryRun { session, .. } => session.as_ref(),
        R::Context { session, .. }
        | R::SessionTurn { session, .. }
        | R::SessionClose { session }
        | R::SessionResolve { session, .. }
        | R::SessionDisplay { session, .. }
        | R::SessionRead { session, .. }
        | R::SessionTaskPolicy { session }
        | R::SessionWiden { session, .. }
        | R::SessionNote { session, .. }
        | R::SessionRecall { session, .. }
        | R::SessionNarrow { session, .. }
        | R::SessionHandles { session }
        | R::MessageSend { session, .. } => Some(session),
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
        | R::ControlStandingRevoke(_) => None,
    }
}
