//! The facts the router gathers for Cedar before it asks: the kill switch, the budgets, the
//! breaker, the limits and the taint. Each is computed here from the router's state and reaches
//! the policy only as a context value; the refusal they bring about is Cedar's ruling.

use action_review::RepeatState;
use docket_core::{Budget, Cost, KillSwitch, Ledger, charge, halted};
use docket_permissions_core::{Limited, Saw};
use policy_point::{BudgetFact, HaltFact, LimitFact, RepeatFact, TaintFact};
use prov::{Effect, SpaceId, SpaceScope, UnixSeconds};

/// What the stateful parts of the router say about one call, before Cedar rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFacts {
    /// Whether the call's Space is halted.
    pub halt: HaltFact,
    /// The scope a refusal for the halt names.
    pub scope: SpaceScope,
    /// Whether the call fits the budgets.
    pub budget: BudgetFact,
    /// The strongest limit that covers it.
    pub limit: LimitFact,
}

/// The budget the call would put over, if any.
pub fn budget_fact(ledger: &Ledger, budget: &Budget, cost: &Cost, now: UnixSeconds) -> BudgetFact {
    charge(ledger, budget, cost, now).map_or_else(BudgetFact::Over, |_| BudgetFact::Within)
}

fn scope_of(space: &SpaceId, switch: &KillSwitch) -> SpaceScope {
    match switch.all {
        docket_core::Halt::Halted { .. } => SpaceScope::Any,
        docket_core::Halt::Running => SpaceScope::Only(space.clone()),
    }
}

fn limit_fact(limited: &Limited) -> LimitFact {
    match limited {
        Limited::Free => LimitFact::Free,
        Limited::Ask(_) => LimitFact::Ask,
        Limited::Never(_) => LimitFact::Never,
    }
}

/// The kill switch, the budget and the limits for `cost` in `space`. The cost is the one before
/// any review is added: a review's own budget is checked when a reviewer would be asked.
pub fn run_facts(
    kill: &KillSwitch,
    space: &SpaceId,
    ledger: &Ledger,
    budget: &Budget,
    cost: &Cost,
    now: UnixSeconds,
    limited: &Limited,
) -> RunFacts {
    RunFacts {
        halt: halted(kill, space).map_or(HaltFact::Running, |_| HaltFact::Halted),
        scope: scope_of(space, kill),
        budget: budget_fact(ledger, budget, cost, now),
        limit: limit_fact(limited),
    }
}

/// The breaker's view of a call as a fact.
pub fn repeat_fact(repeat: RepeatState) -> RepeatFact {
    match repeat {
        RepeatState::Fresh => RepeatFact::Fresh,
        RepeatState::Repeated => RepeatFact::Repeated,
    }
}

/// A session that read untrusted content is tainted for any call that changes something.
pub fn taint_fact(untrusted: Saw, effect: Effect) -> TaintFact {
    match (untrusted, effect) {
        (Saw::Seen, effect) if effect > Effect::Read => TaintFact::Tainted,
        _ => TaintFact::Clean,
    }
}
