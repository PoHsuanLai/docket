//! The router: the seams, the configuration and the state, with one entry point.

use crate::auth::acting_role;
use crate::seams::{Clock, Seams};
use crate::state::RouterState;
use docket_core::{
    AgentConfig, CallerId, IntentsReply, IntentsRequest, WidenAnswer, WidenAsk, WireRefusal,
};
use policy_point::Pdp;
use std::future::Future;
use std::sync::{Mutex, MutexGuard};

/// The router over one set of seams.
#[derive(Debug)]
pub struct Router<S: Seams> {
    /// The seams.
    pub seams: S,
    /// The proposed values, as the settings give them.
    pub config: AgentConfig,
    /// The policy point.
    pub pdp: Pdp,
    /// What it mutates.
    pub state: Mutex<RouterState>,
}

impl<S: Seams> Router<S> {
    /// A router with nothing installed.
    pub fn new(seams: S, config: AgentConfig, pdp: Pdp) -> Self {
        Self {
            seams,
            config,
            pdp,
            state: Mutex::new(RouterState::new()),
        }
    }

    /// The state, even if a thread that held it panicked: the data is still what a test or a
    /// halt needs to read.
    pub(crate) fn locked(&self) -> MutexGuard<'_, RouterState> {
        match self.state.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// Answers one request from one caller. The caller's identity is what the transport
    /// derived; the router checks that its role may make the request (`permits`) before
    /// anything else. The signature is the seam's: a future that can cross a multi-threaded
    /// runtime, written out so every implementation of the seam reads the same.
    #[allow(clippy::manual_async_fn)]
    pub fn handle(
        &self,
        caller: &CallerId,
        request: IntentsRequest,
    ) -> impl Future<Output = IntentsReply> + Send {
        async move {
            let Some(role) = acting_role(&caller.roles, request.member()) else {
                return IntentsReply::Refused(WireRefusal::NotAllowed);
            };
            self.answer(caller, role, request).await
        }
    }

    async fn answer(
        &self,
        caller: &CallerId,
        role: docket_core::CallerRole,
        request: IntentsRequest,
    ) -> IntentsReply {
        use IntentsRequest as R;
        match request {
            R::Manifests => self.manifests(),
            R::IndexPush(batch) => self.index_push(caller, batch),
            R::IndexReset { epoch } => self.index_reset(caller, epoch),
            R::Search(ask) => self.search_query(role, ask).await,
            R::SearchCancel(_) => IntentsReply::Done,
            R::Perform {
                call,
                parent_window,
            } => {
                let now = self.seams.clock().now();
                let who = self.locked().who_for(caller, role, now);
                match who {
                    Ok(who) => IntentsReply::Performed(Box::new(
                        self.perform_chain(who, call, parent_window).await,
                    )),
                    Err(why) => IntentsReply::Refused(why),
                }
            }
            R::Preview(id) => self.run_preview(&id).await,
            R::Suggest(ask) => self.run_suggest(role, ask).await,
            R::Undo(id) => self.run_undo(caller, role, id).await,
            R::UndoAll(scope) => self.run_undo_all(caller, role, scope).await,
            R::Context { session } => self.session_context(&session).await,
            R::SessionOpen(open) => self.session_open(caller, open),
            R::SessionTurn { session, turn } => {
                match self.record_turn(caller, role, &session, turn) {
                    Ok(recorded) => {
                        self.derive_policy(&session).await;
                        IntentsReply::TurnRecorded(recorded.id)
                    }
                    Err(why) => IntentsReply::Refused(why),
                }
            }
            R::SessionClose { session } => self.session_close(caller, role, &session),
            R::SessionResolve { session, handle } => self.session_resolve(&session, handle),
            R::SessionDisplay { session, handle } => self.session_display(&session, handle),
            R::SessionRead { session, ask } => self.session_read(&session, ask).await,
            R::SessionTaskPolicy { session } => match self.locked().sessions.get(&session) {
                Some(record) => IntentsReply::TaskPolicy(record.policy.clone().map(Box::new)),
                None => IntentsReply::Refused(WireRefusal::NoSuchSession),
            },
            R::SessionWiden { session, widen } => self.session_widen(&session, widen).await,
            R::SessionNote { session, note } => self.session_note(&session, note),
            R::SessionRecall { session, ask } => self.session_recall(&session, ask).await,
            R::MessageSend { session, draft } => self.message_send(caller, role, &session, draft),
            R::MessageInbox(ask) => self.message_inbox(role, ask),
            R::GateGrant(ask) => self.gate_grant(ask).await,
            R::GateCheck(ask) => self.gate_check(ask).await,
            R::ControlHalt { scope, cause } => self.control_halt(scope, cause).await,
            R::ControlResume { scope } => self.control_resume(scope),
            R::ControlState => IntentsReply::State(self.locked().kill.clone()),
            R::ControlJournal(filter) => self.control_journal(&filter),
        }
    }

    async fn session_widen(&self, session: &prov::SessionId, widen: WidenAsk) -> IntentsReply {
        let said = self
            .locked()
            .sessions
            .get(session)
            .map(|r| r.turns.iter().any(|t| t.id == widen.turn));
        match said {
            None => IntentsReply::Refused(WireRefusal::NoSuchSession),
            Some(false) => IntentsReply::Refused(WireRefusal::Malformed),
            Some(true) => {
                let answer: WidenAnswer = self.widen_policy(session, widen.change).await;
                IntentsReply::Widened(answer)
            }
        }
    }
}
