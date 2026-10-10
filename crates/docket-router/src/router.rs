//! The router: the seams, the configuration and the state, with one entry point.

use crate::auth::acting_role;
use crate::seams::{Clock, Seams};
use crate::state::RouterState;
use crate::watch::Watch;
use docket_core::{
    AgentConfig, CallerId, CallerRole, IntentsReply, IntentsRequest, WidenAnswer, WidenAsk,
    WireRefusal,
};
use policy_point::Pdp;
use std::future::Future;
use std::sync::{Mutex, MutexGuard, RwLock};

/// The router over one set of seams.
#[derive(Debug)]
pub struct Router<S: Seams> {
    /// The seams.
    pub seams: S,
    /// The proposed values the router starts with.
    pub config: AgentConfig,
    /// The values the person's settings give while the daemon runs (`apply_settings`); they win
    /// over `config` once set.
    live: RwLock<Option<AgentConfig>>,
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
            live: RwLock::new(None),
            pdp,
            state: Mutex::new(RouterState::new()),
        }
    }

    /// The values in force now: the person's latest settings, else the ones the router started
    /// with. Every decision reads them afresh, so a settings change applies to the next call.
    pub fn agent_config(&self) -> AgentConfig {
        match self.live.read() {
            Ok(live) => live.unwrap_or(self.config),
            Err(poisoned) => poisoned.into_inner().unwrap_or(self.config),
        }
    }

    /// Puts the person's settings in force for every call after this one.
    pub fn apply_settings(&self, config: AgentConfig) {
        match self.live.write() {
            Ok(mut live) => *live = Some(config),
            Err(poisoned) => *poisoned.into_inner() = Some(config),
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
    pub fn handle(
        &self,
        caller: &CallerId,
        request: IntentsRequest,
    ) -> impl Future<Output = IntentsReply> + Send {
        self.handle_watched(caller, request, Watch::none())
    }

    /// `handle` for a request whose requester watches it: a computer-use gate check tells
    /// `watch` when it is about to ask the person (`Progress(Confirming)`) and waits for its
    /// `Proceed` before the sheet is drawn, and a withdrawn request takes its sheet back. A
    /// `Run.Perform` tells `watch` how far the call is (`Reviewing`, `Previewing`,
    /// `Confirming(id)`, `Dispatched`) and waits for nothing.
    #[allow(clippy::manual_async_fn)]
    pub fn handle_watched(
        &self,
        caller: &CallerId,
        request: IntentsRequest,
        watch: Watch,
    ) -> impl Future<Output = IntentsReply> + Send {
        async move {
            let Some(role) = acting_role(&caller.roles, request.member()) else {
                return IntentsReply::Refused(WireRefusal::NotAllowed);
            };
            self.restore_named(caller, role, &request).await;
            let reply = self.answer(caller, role, request, &watch).await;
            self.settle(reply).await
        }
    }

    async fn answer(
        &self,
        caller: &CallerId,
        role: docket_core::CallerRole,
        request: IntentsRequest,
        watch: &Watch,
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
                session,
                parent_window,
                activation,
            } => {
                let now = self.seams.clock().now();
                let who = self.locked().who_for(caller, role, session.as_ref(), now);
                // Only the launcher's token means anything: it is the person's own click.
                let activation = activation.filter(|_| role == CallerRole::Launcher);
                match who {
                    // Boxed: the call's whole lifecycle is the largest future the router has, and
                    // it would otherwise sit inline in every caller's.
                    Ok(who) => IntentsReply::Performed(Box::new(
                        Box::pin(self.perform_chain(who, call, parent_window, activation, watch))
                            .await,
                    )),
                    Err(why) => IntentsReply::Refused(why),
                }
            }
            R::DryRun { call, session } => {
                let now = self.seams.clock().now();
                let who = self.locked().who_for(caller, role, session.as_ref(), now);
                match who {
                    Ok(who) => self.run_dry(who, call).await,
                    Err(why) => IntentsReply::Refused(why),
                }
            }
            R::Preview(id) => self.run_preview(&id).await,
            R::Suggest(ask) => self.run_suggest(role, ask).await,
            R::Undo(id) => self.run_undo(caller, role, id).await,
            R::UndoAll(scope) => self.run_undo_all(caller, role, scope).await,
            R::Context { session, app } => {
                // A terminal has one session of its own, which it cannot name.
                let session = match role {
                    CallerRole::Cli => {
                        let now = self.seams.clock().now();
                        match self.locked().who_for(caller, role, None, now) {
                            Ok(who) => who.session.unwrap_or(session),
                            Err(why) => return IntentsReply::Refused(why),
                        }
                    }
                    _ => session,
                };
                self.session_context(&session, app).await
            }
            R::SessionOpen(open) => self.session_open(caller, role, open),
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
            R::SessionResolve { session, handle } => self.session_resolve(&session, handle).await,
            R::SessionDisplay { session, handle } => self.session_display(&session, handle),
            R::SessionDisplayLabelled { session, handle } => {
                self.session_display_labelled(&session, handle)
            }
            R::SessionRead { session, ask } => self.session_read(&session, ask).await,
            R::SessionTaskPolicy { session } => match self.locked().sessions.get(&session) {
                Some(record) => IntentsReply::TaskPolicy(record.policy.clone().map(Box::new)),
                None => IntentsReply::Refused(WireRefusal::NoSuchSession),
            },
            R::SessionWiden { session, widen } => self.session_widen(&session, widen).await,
            R::SessionNote { session, note } => self.session_note(&session, note),
            R::SessionRecall { session, ask } => self.session_recall(&session, ask).await,
            R::SessionNarrow { session, turn } => self.session_narrow(&session, turn).await,
            R::SessionHandles { session } => self.session_handles(&session),
            R::SessionStored { ask } => self.session_stored(caller, role, ask).await,
            R::MessageSend { session, draft } => self.message_send(caller, role, &session, draft),
            R::MessageInbox(ask) => self.message_inbox(role, ask).await,
            R::GateGrant(ask) => self.gate_grant(ask).await,
            R::GateCheck(ask) => self.gate_check(ask, watch).await,
            R::ControlHalt { scope, cause } => self.control_halt(scope, cause).await,
            R::ControlResume { scope } => self.control_resume(scope),
            R::ControlState => IntentsReply::State(self.locked().kill.clone()),
            R::ControlJournal(filter) => self.control_journal(role, &filter),
            R::ControlTerminalGrants => IntentsReply::TerminalGrants(self.terminal_grants()),
            R::ControlTerminalRevoke(action) => {
                self.revoke_terminal_grant(&action);
                IntentsReply::Done
            }
            R::ControlStandingGrants => IntentsReply::StandingGrants(self.standing_grants()),
            R::ControlStandingRevoke(id) => {
                self.revoke_standing(&id);
                IntentsReply::Done
            }
            // The restore points are not served yet; refused, never guessed.
            R::CheckpointList { .. } | R::CheckpointPlan { .. } => {
                IntentsReply::Refused(WireRefusal::Malformed)
            }
            // A request this router does not know is refused, never run.
            _ => IntentsReply::Refused(WireRefusal::Malformed),
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
