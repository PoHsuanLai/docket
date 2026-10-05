//! Budgets, the ledger that spends them, and the kill switch. `charge` and `halted` are pure.

use crate::review::BreakerTrip;
use crate::units::{Depth, Seconds};
use porter_core::{Count, MicroUsd};
use prov::{Effect, SpaceId, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What one session may spend (settings `agent.budget.*`, proposed defaults). The denial limit
/// is the breaker's, not a budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Budget {
    /// Calls per session (200).
    pub calls: Count,
    /// Writes of any kind (100).
    pub writes: Count,
    /// Outbound acts (10).
    pub outbound: Count,
    /// Destructive acts (5).
    pub destructive: Count,
    /// Calls per minute (30).
    pub per_minute: Count,
    /// Things one call may touch (50).
    pub fan_out: Count,
    /// How deep a follow-up chain may go (4).
    pub chain: Depth,
    /// Wall time (1800 s).
    pub wall: Seconds,
    /// Reviews (300).
    pub reviews: Count,
    /// Spend, shared with inferd's cap.
    pub spend: MicroUsd,
}

/// Which budget ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetKind {
    /// Calls.
    Calls,
    /// Writes.
    Writes,
    /// Outbound acts.
    Outbound,
    /// Destructive acts.
    Destructive,
    /// Calls per minute.
    Rate,
    /// Things per call.
    FanOut,
    /// Chain depth.
    Chain,
    /// Wall time.
    Wall,
    /// Reviews.
    Reviews,
    /// Money.
    Spend,
}

/// What a session has used so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Ledger {
    /// When the session started.
    pub started: UnixSeconds,
    /// Calls so far.
    pub calls: Count,
    /// Writes so far.
    pub writes: Count,
    /// Outbound acts so far.
    pub outbound: Count,
    /// Destructive acts so far.
    pub destructive: Count,
    /// Reviews so far.
    pub reviews: Count,
    /// When the current one-minute window opened.
    pub window_start: UnixSeconds,
    /// Calls in the current window.
    pub in_window: Count,
}

impl Ledger {
    /// A session that starts at `now` and has used nothing.
    pub fn new(now: UnixSeconds) -> Self {
        Self {
            started: now,
            calls: Count(0),
            writes: Count(0),
            outbound: Count(0),
            destructive: Count(0),
            reviews: Count(0),
            window_start: now,
            in_window: Count(0),
        }
    }
}

/// Whether a call used a review.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reviewed {
    /// No.
    No,
    /// Yes.
    Yes,
}

/// What one call costs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Cost {
    /// Its effect.
    pub effect: Effect,
    /// How many things it touches.
    pub entities: Count,
    /// How deep in a chain.
    pub depth: Depth,
    /// Whether it was reviewed.
    pub review: Reviewed,
}

const WINDOW_SECONDS: i64 = 60;

/// Spends one call from the ledger, or names the budget that is out. The wall budget counts from
/// the session's start; the per-minute budget counts calls in a one-minute window.
pub fn charge(
    ledger: &Ledger,
    budget: &Budget,
    cost: &Cost,
    now: UnixSeconds,
) -> Result<Ledger, BudgetKind> {
    let over = |used: Count, limit: Count| used.0.saturating_add(1) > limit.0;
    let fresh = now.0 - ledger.window_start.0 >= WINDOW_SECONDS;
    let (window_start, in_window) = if fresh {
        (now, Count(0))
    } else {
        (ledger.window_start, ledger.in_window)
    };
    let write = cost.effect > Effect::Read;
    let outbound = cost.effect == Effect::Outbound;
    let destructive = cost.effect == Effect::Destructive;
    let reviewed = cost.review == Reviewed::Yes;
    if now.0 - ledger.started.0 > i64::from(budget.wall.0) {
        Err(BudgetKind::Wall)
    } else if cost.depth > budget.chain {
        Err(BudgetKind::Chain)
    } else if cost.entities > budget.fan_out {
        Err(BudgetKind::FanOut)
    } else if over(ledger.calls, budget.calls) {
        Err(BudgetKind::Calls)
    } else if over(in_window, budget.per_minute) {
        Err(BudgetKind::Rate)
    } else if write && over(ledger.writes, budget.writes) {
        Err(BudgetKind::Writes)
    } else if outbound && over(ledger.outbound, budget.outbound) {
        Err(BudgetKind::Outbound)
    } else if destructive && over(ledger.destructive, budget.destructive) {
        Err(BudgetKind::Destructive)
    } else if reviewed && over(ledger.reviews, budget.reviews) {
        Err(BudgetKind::Reviews)
    } else {
        let step = |n: Count, on: bool| Count(n.0.saturating_add(u32::from(on)));
        Ok(Ledger {
            started: ledger.started,
            calls: step(ledger.calls, true),
            writes: step(ledger.writes, write),
            outbound: step(ledger.outbound, outbound),
            destructive: step(ledger.destructive, destructive),
            reviews: step(ledger.reviews, reviewed),
            window_start,
            in_window: step(in_window, true),
        })
    }
}

/// Why everything stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum HaltCause {
    /// The hardware kill chord.
    KillChord,
    /// The control centre.
    ControlCentre,
    /// The stop key (⌘.).
    StopKey,
    /// A budget ran out.
    Budget(BudgetKind),
    /// The breaker tripped.
    Breaker(BreakerTrip),
}

/// Whether a scope is running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Halt {
    /// Running.
    Running,
    /// Stopped since then, for this cause.
    Halted {
        /// When.
        since: UnixSeconds,
        /// Why.
        by: HaltCause,
    },
}

/// The global halt and the per-Space halts. A global halt overrides every Space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillSwitch {
    /// Every Space.
    pub all: Halt,
    /// One Space at a time.
    pub spaces: BTreeMap<SpaceId, Halt>,
    /// Audit records lost since intentd started: memoryd refused them (a Space it does not know)
    /// or the queue was full. Zero while the log is whole.
    #[serde(default = "no_records")]
    pub audit_lost: Count,
}

fn no_records() -> Count {
    Count(0)
}

/// The cause the Space is halted for, if it is: the global halt first.
pub fn halted(switch: &KillSwitch, space: &SpaceId) -> Option<HaltCause> {
    let cause = |h: &Halt| match h {
        Halt::Running => None,
        Halt::Halted { by, .. } => Some(*by),
    };
    cause(&switch.all).or_else(|| switch.spaces.get(space).and_then(cause))
}
