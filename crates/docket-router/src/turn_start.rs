//! The one read a turn start may run unasked: the companion looks up memory for the words the
//! person just typed, before its first planner call. The call is made as any other (gated,
//! counted, audited, labelled), and only the sheet that would ask about a first use is skipped.
//!
//! The exception is typed and narrow. The caller says `Origin::TurnStart`; the router then
//! checks everything itself: the action is `memory.recall` of the memory app, the caller is the
//! main planner of its session (never a worker, a reader or a computer-use run), and the
//! session's newest turn was recorded by a launcher, a field or an editor (the person's own
//! surfaces; a terminal and an external agent's host are not). A claim that fails the checks is
//! refused outright, never turned into a question. A recorded denial of the person still stands.

use crate::who::Who;
use docket_core::{
    CallRequest, CallerRole, Origin, RECALL_ACTION, RECALL_APP, TurnSource, UserTurn,
};
use porter_core::GrantId;
use porter_core::consent::{GrantScope, Verdict};
use prov::{Actor, AgentRole};

/// What the router makes of a call's claim to be a turn start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnStart {
    /// The call does not claim it: the usual consent applies.
    Unclaimed,
    /// The claim holds: first use needs no sheet.
    Preapproved,
    /// The claim does not hold: the call is refused.
    Refused,
}

fn person_typed(turns: &[UserTurn]) -> bool {
    matches!(
        turns.last().map(|t| &t.from),
        Some(TurnSource::Launcher | TurnSource::Field(_) | TurnSource::Editor(_))
    )
}

/// Rules on a call's claim to be a turn start.
pub(crate) fn rule(who: &Who, request: &CallRequest, turns: &[UserTurn]) -> TurnStart {
    if request.origin != Origin::TurnStart {
        return TurnStart::Unclaimed;
    }
    let the_search =
        request.action.app.as_str() == RECALL_APP && request.action.name.as_str() == RECALL_ACTION;
    let main_planner = who.role == CallerRole::Companion
        && matches!(
            who.actor,
            Actor::Companion {
                role: AgentRole::Planner,
                ..
            }
        );
    if the_search && main_planner && person_typed(turns) {
        TurnStart::Preapproved
    } else {
        TurnStart::Refused
    }
}

/// The consent of a pre-approved turn start: what standing consent said, except that a question
/// becomes a pass. A denial the person recorded is kept.
pub(crate) fn pass(verdict: Verdict) -> Verdict {
    match (verdict, GrantId::parse("g-none")) {
        (Verdict::Ask, Ok(grant)) => Verdict::Granted {
            grant,
            scope: GrantScope::Always,
        },
        (other, _) => other,
    }
}
