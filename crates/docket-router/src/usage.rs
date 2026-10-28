//! Model spend: what a task's model turns used, counted in the session's ledger beside its
//! calls, and checked before the next turn.

use crate::router::Router;
use crate::seams::Seams;
use docket_core::{BudgetKind, TurnReport, charge_turn, check_turn};
use docket_gate::spans;
use docket_session::SessionEntry;
use prov::SessionId;

/// Where a session's spend stands after a turn was counted, or before one starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TurnBudget {
    /// The session may take another turn.
    Within,
    /// It may not: this budget is out (`Spend`: the reported charges have gone over the cap).
    Over(BudgetKind),
    /// The router holds no such session.
    NoSession,
}

impl<S: Seams> Router<S> {
    /// Counts one model turn against the session: its tokens and the charge the provider
    /// reported, if it did (a turn with none is counted as unpriced). The turn is recorded even if it takes
    /// the session over its cap (the money is spent); the answer says whether another turn may
    /// start. The ledger is written to the session's record so a restore keeps the spend.
    pub fn note_turn_usage(&self, session: &SessionId, report: &TurnReport) -> TurnBudget {
        let budget = self.agent_config().budget;
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(session) else {
            return TurnBudget::NoSession;
        };
        record.ledger = charge_turn(&record.ledger, report);
        record.wal.note(SessionEntry::Budget(record.ledger));
        let task = record.task.clone();
        let verdict = standing(check_turn(&record.ledger, &budget));
        drop(st);
        drop(spans::invoke_agent(session, &task, report));
        verdict
    }

    /// Whether the session may start another model turn: asked before the model is.
    pub fn turn_budget(&self, session: &SessionId) -> TurnBudget {
        let budget = self.agent_config().budget;
        match self.locked().sessions.get(session) {
            Some(record) => standing(check_turn(&record.ledger, &budget)),
            None => TurnBudget::NoSession,
        }
    }
}

fn standing(checked: Result<(), BudgetKind>) -> TurnBudget {
    match checked {
        Ok(()) => TurnBudget::Within,
        Err(kind) => TurnBudget::Over(kind),
    }
}
