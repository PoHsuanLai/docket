//! Carrying a call out: its class grant, its undo entry, its place in the session's ledger and
//! the dispatch to the app.

use crate::durable::Flush;
use crate::router::Router;
use crate::seams::{AppFault, AppLink, GrantStore, Seams};
use docket_core::{BudgetKind, CallEnd, CallRefusal, Ledger, Outcome, charge};
use docket_gate::brief_sheet::is_brief_confirm;
use docket_gate::call::{CallEvent, CallState};
use docket_gate::driven::{Next, Run};
use docket_gate::prepared::{Prepared, grants_for};
use docket_session::{CallOpen, SessionEntry};
use docket_vocab::{Clock, UndoId};
use porter_core::GrantId;
use prov::{Actor, AgentRole, SessionId};

/// A ledger checkpoint is written when the session has made a multiple of this many calls.
const BUDGET_EVERY: u32 = 5;

impl<S: Seams> Router<S> {
    /// The broad "always" of the other callers: one grant per data class, for the app in this
    /// Space.
    pub(crate) fn record_class_grant(&self, p: &Prepared) {
        let now = self.seams.clock().now();
        let id = {
            let n = self.locked().mint();
            GrantId::parse(&format!("g-{n}")).ok()
        };
        if let Some(id) = id {
            for grant in grants_for(p, id, now, p.who.grant_caller()) {
                self.seams.grants().record(grant);
            }
        }
    }

    pub(crate) fn journal_undo(
        &self,
        p: &Prepared,
        outcome: &Outcome,
        token: &docket_vocab::UndoToken,
    ) -> UndoId {
        let now = self.seams.clock().now();
        let said = outcome.said.clone().unwrap_or_else(|| p.decl.label.clone());
        let (run_id, session) = match (&p.who.actor, &p.who.session) {
            (
                Actor::Companion {
                    role: AgentRole::Cua { run },
                    ..
                },
                session,
            ) => (Some(run.clone()), session.clone()),
            (_, session) => (None, session.clone()),
        };
        self.locked().journal.record(
            now,
            p.who.actor.clone(),
            p.request.action.clone(),
            said,
            token.clone(),
            run_id,
            session,
        )
    }

    /// Charges the call to its session and queues its `Call` entry. Gives back the ledger as it
    /// was, to restore if the call cannot be recorded and so does not run.
    pub(crate) fn begin_call(
        &self,
        p: &Prepared,
        reviewed: bool,
        session: &SessionId,
    ) -> Result<Option<Ledger>, BudgetKind> {
        let now = self.seams.clock().now();
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(session) else {
            return Ok(None);
        };
        let cost = docket_core::Cost { reviewed, ..p.cost };
        let ledger = charge(&record.ledger, &self.agent_config().budget, &cost, now)?;
        let before = std::mem::replace(&mut record.ledger, ledger);
        // The turn-start memory read is a read the companion brought as context: nothing to
        // recover after a crash, and no step a replay shows (see `finish`).
        if p.request.origin != docket_vocab::Origin::TurnStart {
            record.wal.note(SessionEntry::Call(CallOpen {
                call: p.id,
                action: p.request.action.clone(),
                effect: p.decl.effect,
            }));
        }
        if record.ledger.calls.0 % BUDGET_EVERY == 0 {
            record.wal.note(SessionEntry::Budget(record.ledger));
        }
        Ok(Some(before))
    }

    pub(crate) async fn dispatch(&self, p: &Prepared, run: &mut Run) -> Next {
        let answered = run.receipt.is_some();
        if self.standing_use(p, answered).is_err() {
            run.state = CallState::Done(CallEnd::Refused(CallRefusal::Denied(
                docket_permissions_core::DenyCode::NeedsUser,
            )));
            return Next::Stop;
        }
        let reviewed = !run.verdicts.is_empty();
        if let Some(session) = &p.who.session {
            let before = match self.begin_call(p, reviewed, session) {
                Ok(before) => before,
                Err(kind) => {
                    run.state = CallState::Done(CallEnd::Refused(CallRefusal::OverBudget(kind)));
                    return Next::Stop;
                }
            };
            // The call is on the record before it is handed to the app: a crash after this
            // leaves a `Call` with no `Step`, which a restore ends interrupted and never runs
            // again. If it cannot be recorded, it does not run.
            if self.flush(session).await != Flush::Durable {
                if let Some(record) = self.locked().sessions.get_mut(session) {
                    record.wal.retract_call(p.id);
                    if let Some(before) = before {
                        record.ledger = before;
                    }
                }
                run.state = CallState::Done(CallEnd::Refused(CallRefusal::NotRecorded));
                return Next::Stop;
            }
        }
        let answer = if is_brief_confirm(&p.request.action) {
            // Only the router sees the words the person confirmed, so no app is asked.
            self.confirmed_brief(p, run).map_err(AppFault::Refused)
        } else {
            self.seams
                .link()
                .perform_classified(
                    &p.request.action.app,
                    self.invocation(p),
                    p.activation.clone(),
                    p.classified.as_ref().map(|c| c.sent.clone()),
                    p.decl.latency,
                )
                .await
                .and_then(|outcome| self.related_outcome(p, outcome).map_err(AppFault::Refused))
        };
        run.outcome = answer.as_ref().ok().cloned();
        match answer {
            Err(AppFault::TimedOut) => Next::Event(CallEvent::AppTimedOut),
            Err(AppFault::Unavailable) => {
                Next::Event(CallEvent::AppUnavailable(p.request.action.app.clone()))
            }
            Ok(outcome) => Next::Event(CallEvent::AppAnswered(Box::new(Ok(outcome)))),
            Err(AppFault::Refused(why)) => Next::Event(CallEvent::AppAnswered(Box::new(Err(why)))),
        }
    }
}
