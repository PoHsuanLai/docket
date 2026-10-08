//! How a turn ended, from where the task's loop stands: pure.

use crate::task::Failure;
use agent_loop::{FinishedAs, LoopPhase, LoopState};
use docket_planner::PlanFault;
use docket_session::TurnEnd;

/// The turn's end for a loop that has run as far as it can alone. A loop at rest on a question
/// for the person (`Idle`) ended its turn in good order: the question went out as an event. One
/// still planning or waiting on calls was cut short, which is a failure.
pub(crate) fn end_of(state: Option<&LoopState>, failure: Option<&Failure>) -> TurnEnd {
    match state.map(|s| s.phase) {
        Some(LoopPhase::Finished(FinishedAs::Done) | LoopPhase::Idle) => TurnEnd::Done,
        Some(LoopPhase::Finished(FinishedAs::Cancelled)) => TurnEnd::Cancelled,
        Some(LoopPhase::Finished(FinishedAs::Failed)) => failed(failure),
        Some(LoopPhase::Paused(trip)) => TurnEnd::Paused(trip),
        Some(LoopPhase::Planning | LoopPhase::AwaitingCalls | LoopPhase::AwaitingReader) | None => {
            TurnEnd::Failed
        }
    }
}

/// A model that declined, or a call refused for the budget, is the backend saying no; anything
/// else is a failure.
fn failed(failure: Option<&Failure>) -> TurnEnd {
    match failure {
        Some(Failure::Model(PlanFault::Declined(_)) | Failure::Refused(_)) => TurnEnd::Refused,
        Some(Failure::Model(_) | Failure::Reader | Failure::Budget) | None => TurnEnd::Failed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::BreakerTrip;
    use docket_core::{BudgetKind, CallRefusal};

    fn at(phase: LoopPhase) -> LoopState {
        LoopState {
            phase,
            turn: None,
            steps: 0,
            pending: vec![],
            guard: Default::default(),
        }
    }

    #[test]
    fn the_end_follows_the_loop() {
        let trip = BreakerTrip::Consecutive;
        let table = [
            (
                Some(LoopPhase::Finished(FinishedAs::Done)),
                None,
                TurnEnd::Done,
            ),
            (Some(LoopPhase::Idle), None, TurnEnd::Done),
            (
                Some(LoopPhase::Finished(FinishedAs::Cancelled)),
                None,
                TurnEnd::Cancelled,
            ),
            (
                Some(LoopPhase::Finished(FinishedAs::Failed)),
                None,
                TurnEnd::Failed,
            ),
            (
                Some(LoopPhase::Finished(FinishedAs::Failed)),
                Some(Failure::Budget),
                TurnEnd::Failed,
            ),
            (
                Some(LoopPhase::Finished(FinishedAs::Failed)),
                Some(Failure::Refused(CallRefusal::OverBudget(BudgetKind::Calls))),
                TurnEnd::Refused,
            ),
            (Some(LoopPhase::Paused(trip)), None, TurnEnd::Paused(trip)),
            (Some(LoopPhase::Planning), None, TurnEnd::Failed),
            (Some(LoopPhase::AwaitingCalls), None, TurnEnd::Failed),
            (None, None, TurnEnd::Failed),
        ];
        for (phase, failure, want) in table {
            let state = phase.map(at);
            assert_eq!(end_of(state.as_ref(), failure.as_ref()), want, "{phase:?}");
        }
    }
}
