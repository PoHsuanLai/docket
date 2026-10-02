//! The pure decision that sits before dispatch: the order in which a call is refused, sent to
//! review, sent to the person, or let through (call lifecycle §4.1, with the addendum's rows).

use action_review::{RepeatState, plan};
use docket_core::{
    AskReason, Budget, CallRefusal, Cost, DenyCode, Impact, KillSwitch, Ledger, Ruling, Stage,
    charge, halted,
};
use porter_core::consent::Verdict;
use prov::{SpaceId, SpaceScope, UnixSeconds};

/// What the router does next.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    /// Dispatch it.
    Run,
    /// A reviewer may tighten it: these stages.
    NeedsReview(Vec<Stage>),
    /// Ask the person, for these reasons (after the app previews the change).
    Confirm(Vec<AskReason>),
    /// Refuse it.
    Refuse(CallRefusal),
}

/// Everything the decision reads.
#[derive(Debug, Clone, Copy)]
pub struct GateInputs<'a> {
    /// The kill switch.
    pub halt: &'a KillSwitch,
    /// The call's Space.
    pub space: &'a SpaceId,
    /// What the session has used.
    pub ledger: &'a Ledger,
    /// What it may use.
    pub budget: &'a Budget,
    /// What this call costs.
    pub cost: &'a Cost,
    /// Now.
    pub now: UnixSeconds,
    /// What standing consent says.
    pub consent: &'a Verdict,
    /// What policy ruled.
    pub ruling: &'a Ruling,
    /// How much is at stake.
    pub impact: Impact,
    /// Whether the exact call was denied before.
    pub repeat: RepeatState,
}

fn scope_of(space: &SpaceId, switch: &KillSwitch) -> SpaceScope {
    match switch.all {
        docket_core::Halt::Halted { .. } => SpaceScope::Any,
        docket_core::Halt::Running => SpaceScope::Only(space.clone()),
    }
}

/// Decides, in this order: a halt, a budget, an exact repeat of a denial, a consent denial,
/// then the ruling (`Deny` refuses; `Ask`, or consent still to ask, confirms; `AllowJudged`
/// goes to review; `AllowFinal` runs). The ledger is not charged here; the router charges when
/// it dispatches.
pub fn gate(i: &GateInputs<'_>) -> Pending {
    if halted(i.halt, i.space).is_some() {
        return Pending::Refuse(CallRefusal::Halted(scope_of(i.space, i.halt)));
    }
    if let Err(kind) = charge(i.ledger, i.budget, i.cost, i.now) {
        return Pending::Refuse(CallRefusal::OverBudget(kind));
    }
    if i.repeat == RepeatState::Repeated {
        return Pending::Refuse(CallRefusal::Denied(DenyCode::Repeated));
    }
    if *i.consent == Verdict::Denied {
        return Pending::Refuse(CallRefusal::Denied(DenyCode::NotAllowed));
    }
    let first_use = matches!(i.consent, Verdict::Ask);
    match i.ruling {
        Ruling::Deny(_) => Pending::Refuse(CallRefusal::Denied(DenyCode::NotAllowed)),
        Ruling::Ask(why) => {
            let mut why = why.clone();
            if first_use && !why.contains(&AskReason::FirstUse) {
                why.push(AskReason::FirstUse);
            }
            Pending::Confirm(why)
        }
        _ if first_use => Pending::Confirm(vec![AskReason::FirstUse]),
        Ruling::AllowJudged(_) => Pending::NeedsReview(plan(i.ruling, i.impact)),
        Ruling::AllowFinal(_) => Pending::Run,
    }
}
