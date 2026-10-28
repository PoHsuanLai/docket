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
//!
//! The same pass covers the main planner's own `memory.recall` and `memory.facts` on such a turn:
//! reading the person's own memory for their own request does not ask either.

use docket_context::who::Who;
use docket_core::UserTurn;
use docket_vocab::{CallRequest, CallerRole, Origin, RECALL_ACTION, RECALL_APP};
use porter_core::GrantId;
use porter_core::consent::{GrantScope, Verdict};
use prov::{Actor, AgentRole};

/// What the router makes of a call's claim to be a turn start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStart {
    /// The call does not claim it: the usual consent applies.
    Unclaimed,
    /// The claim holds: first use needs no sheet.
    Preapproved,
    /// The claim does not hold: the call is refused.
    Refused,
}

fn person_typed(turns: &[UserTurn]) -> bool {
    turns.last().is_some_and(|t| {
        matches!(
            t.from.role(),
            CallerRole::Launcher | CallerRole::Field | CallerRole::Editor
        )
    })
}

/// The memory app's reads of the person's own memory that the main planner may make unasked
/// while it works on a request the person typed (the owner's decision: reading their own
/// memory for their own question needs no sheet). Writes and every other app still ask.
const OWN_READS: [&str; 2] = [RECALL_ACTION, "memory.facts"];

/// Rules on a call's claim to be a turn start, and on the planner's own reads of memory.
pub fn rule(who: &Who, request: &CallRequest, turns: &[UserTurn]) -> TurnStart {
    let memory = request.action.app.as_str() == RECALL_APP;
    let main_planner = who.role == CallerRole::Companion
        && matches!(
            who.actor,
            Actor::Companion {
                role: AgentRole::Planner,
                ..
            }
        );
    let allowed = main_planner && person_typed(turns);
    match request.origin {
        Origin::TurnStart if memory && request.action.name.as_str() == RECALL_ACTION && allowed => {
            TurnStart::Preapproved
        }
        Origin::TurnStart => TurnStart::Refused,
        Origin::Companion
            if memory && OWN_READS.contains(&request.action.name.as_str()) && allowed =>
        {
            TurnStart::Preapproved
        }
        _ => TurnStart::Unclaimed,
    }
}

/// The consent of a pre-approved turn start: what standing consent said, except that a question
/// becomes a pass. A denial the person recorded is kept.
pub fn pass(verdict: Verdict) -> Verdict {
    match (verdict, GrantId::parse("g-none")) {
        (Verdict::Ask, Ok(grant)) => Verdict::Granted {
            grant,
            scope: GrantScope::Always,
        },
        (other, _) => other,
    }
}
