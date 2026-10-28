//! The pure mapping from what policy ruled to what the router does next (call lifecycle §4.1).
//! Cedar decides everything: the halt, the budgets, an exact repeat, a limit and a recorded
//! denial reach it as facts (`facts.rs`) and come back as a ruling. This file only words a
//! refusal and picks the next step; it never overrides a ruling.

use crate::facts::RunFacts;
use action_review::plan;
use docket_core::CallRefusal;
use docket_permissions_core::{
    AskReason, DenyCode, Impact, PolicyId, Ruling, Stage, StandingGrantId,
};
use policy_point::BudgetFact;

/// The core rules whose Deny the router words by their facts. Their ids are `policy/core.cedar`'s.
const HALTED: &str = "halted";
const OVER_BUDGET: &str = "over-budget";
const REPEATED: &str = "repeated";
const LIMIT_NEVER: &str = "limit-never";

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

/// Everything the mapping reads.
#[derive(Debug, Clone, Copy)]
pub struct GateInputs<'a> {
    /// What policy ruled.
    pub ruling: &'a Ruling,
    /// How much is at stake.
    pub impact: Impact,
    /// The facts a refusal is worded from (the halt's scope, the budget that ran out).
    pub facts: &'a RunFacts,
    /// Whether the call, with the review it needs, fits the budgets: a reviewer is asked only
    /// while the reviews budget has room.
    pub charged: BudgetFact,
}

fn named(ids: &[PolicyId], rule: &str) -> bool {
    ids.iter().any(|id| id.0 == rule)
}

/// Whether `id` is a "never" that said so: the core rule, or a rule written for a limit (named by
/// the limit's id, which a standing grant's id shares the form of).
fn says_never(id: &PolicyId) -> bool {
    id.0 == LIMIT_NEVER || StandingGrantId::parse(&id.0).is_ok()
}

/// The refusal for the rules that denied: a halt first, then a budget, a repeat, and any other
/// rule as "not allowed"; a limit's "never" only when nothing else denied.
fn refusal(ids: &[PolicyId], facts: &RunFacts) -> CallRefusal {
    let other = ids.iter().any(|id| !says_never(id));
    match facts.budget {
        _ if named(ids, HALTED) => CallRefusal::Halted(facts.scope.clone()),
        BudgetFact::Over(kind) if named(ids, OVER_BUDGET) => CallRefusal::OverBudget(kind),
        _ if named(ids, REPEATED) => CallRefusal::Denied(DenyCode::Repeated),
        _ if !other && ids.iter().any(says_never) => CallRefusal::Denied(DenyCode::Never),
        _ => CallRefusal::Denied(DenyCode::NotAllowed),
    }
}

/// `Deny` refuses; `Ask` confirms; `AllowJudged` goes to review (unless the reviews budget is
/// spent); `AllowFinal` runs. The ledger is not charged here; the router charges when it
/// dispatches.
pub fn gate(i: &GateInputs<'_>) -> Pending {
    match (i.ruling, i.charged) {
        (Ruling::Deny(ids), _) => Pending::Refuse(refusal(ids, i.facts)),
        (Ruling::Ask(why), _) => Pending::Confirm(why.clone()),
        (Ruling::AllowJudged(_), BudgetFact::Over(kind)) => {
            Pending::Refuse(CallRefusal::OverBudget(kind))
        }
        (Ruling::AllowJudged(_), BudgetFact::Within) => {
            Pending::NeedsReview(plan(i.ruling, i.impact))
        }
        (Ruling::AllowFinal(_), _) => Pending::Run,
    }
}
