//! The denial circuit breaker: three denials in a row, ten of the last fifty decisions, or the
//! same goal tried with different arguments, stop the session until the person speaks. Pure.

use docket_core::{BreakerLimits, BreakerTrip};
use porter_core::{AppName, Count};
use prov::{ActionName, EntityKind, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// What a call was trying to do, arguments excluded.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GoalKey {
    /// The app.
    pub app: AppName,
    /// The action.
    pub action: ActionName,
    /// The kind it acts on, if any.
    pub kind: Option<EntityKind>,
}

/// A digest of a call's arguments, computed by the router: two calls with the same digest are
/// the same call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ArgDigest(pub u64);

/// Who denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeniedBy {
    /// Policy.
    Policy,
    /// A reviewer.
    Reviewer,
    /// The person.
    User,
}

/// One denial.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DenialMark {
    /// When.
    pub at: UnixSeconds,
    /// The goal.
    pub goal: GoalKey,
    /// The arguments, by digest.
    pub args: ArgDigest,
    /// Who denied.
    pub by: DeniedBy,
}

/// One decision of the session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum DecisionMark {
    /// A call was allowed.
    Allowed,
    /// A call was denied.
    Denied(DenialMark),
}

/// The breaker's memory of one session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breaker {
    /// Denials since the last allow.
    pub consecutive: Count,
    /// The last decisions, oldest first, at most the window.
    pub recent: Vec<DecisionMark>,
}

impl Breaker {
    /// A session that has decided nothing. A resume after a trip starts here too.
    pub fn new() -> Self {
        Self {
            consecutive: Count(0),
            recent: Vec::new(),
        }
    }

    fn denials(&self) -> impl Iterator<Item = &DenialMark> {
        self.recent.iter().filter_map(|d| match d {
            DecisionMark::Allowed => None,
            DecisionMark::Denied(m) => Some(m),
        })
    }
}

impl Default for Breaker {
    fn default() -> Self {
        Self::new()
    }
}

/// Whether a call exactly repeats one that was denied.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatState {
    /// New.
    Fresh,
    /// The same goal with the same arguments was denied earlier: refused without review.
    Repeated,
}

/// Whether some goal was denied with at least `probing` different arguments.
fn probing(breaker: &Breaker, probing: Count) -> bool {
    let goals: BTreeSet<&GoalKey> = breaker.denials().map(|m| &m.goal).collect();
    goals.into_iter().any(|goal| {
        let distinct: BTreeSet<ArgDigest> = breaker
            .denials()
            .filter(|m| &m.goal == goal)
            .map(|m| m.args)
            .collect();
        distinct.len() >= probing.0 as usize
    })
}

/// Notes a decision and says whether the breaker trips. An allow resets the consecutive count
/// but not the recent window. When several limits hold, probing is reported first (the most
/// specific signal), then consecutive, then recent.
pub fn note(
    breaker: Breaker,
    decision: DecisionMark,
    limits: &BreakerLimits,
) -> (Breaker, Option<BreakerTrip>) {
    let consecutive = match decision {
        DecisionMark::Allowed => Count(0),
        DecisionMark::Denied(_) => Count(breaker.consecutive.0.saturating_add(1)),
    };
    let mut recent = breaker.recent;
    recent.push(decision);
    let keep = limits.window.0 as usize;
    if recent.len() > keep {
        recent.drain(..recent.len() - keep);
    }
    let next = Breaker {
        consecutive,
        recent,
    };
    let denied_recent = next.denials().count();
    let trip = if probing(&next, limits.probing) {
        Some(BreakerTrip::Probing)
    } else if next.consecutive >= limits.consecutive {
        Some(BreakerTrip::Consecutive)
    } else if denied_recent >= limits.recent.0 as usize {
        Some(BreakerTrip::Recent)
    } else {
        None
    };
    (next, trip)
}

/// Whether `goal` with `args` was already denied in this session.
pub fn repeated(breaker: &Breaker, goal: &GoalKey, args: ArgDigest) -> RepeatState {
    if breaker.denials().any(|m| &m.goal == goal && m.args == args) {
        RepeatState::Repeated
    } else {
        RepeatState::Fresh
    }
}
