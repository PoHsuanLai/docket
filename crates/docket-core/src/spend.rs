//! What a session's model turns have cost: tokens by kind, money, and whether the money is
//! known. Pure; `budget::charge` reads it and `charge_turn` writes it.

use crate::budget::{Budget, BudgetKind, Ledger};
use porter_core::MicroUsd;
use porter_ids::Tokens;
use serde::{Deserialize, Serialize};

/// Tokens by kind. `cached` is a share of `input`, as the model reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TokenTotals {
    /// Prompt tokens.
    pub input: Tokens,
    /// Reply tokens.
    pub output: Tokens,
    /// Prompt tokens served from the model's cache.
    pub cached: Tokens,
}

impl TokenTotals {
    /// No tokens.
    pub const NONE: Self = Self {
        input: Tokens(0),
        output: Tokens(0),
        cached: Tokens(0),
    };

    /// These and `more`, never wrapping.
    pub fn plus(self, more: Self) -> Self {
        let add = |a: Tokens, b: Tokens| Tokens(a.0.saturating_add(b.0));
        Self {
            input: add(self.input, more.input),
            output: add(self.output, more.output),
            cached: add(self.cached, more.cached),
        }
    }
}

/// Whether the money counted is the whole bill.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Certainty {
    /// Every turn reported its usage, or was charged its worst case.
    Known,
    /// A turn reported nothing and no worst case is known: the cap cannot be trusted.
    Uncertain,
}

/// What a session's model turns have used so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Spent {
    /// Tokens.
    pub tokens: TokenTotals,
    /// Money, in millionths of a US dollar.
    pub cost: MicroUsd,
    /// Whether `cost` is the whole bill.
    pub certainty: Certainty,
}

impl Spent {
    /// Nothing used yet.
    pub const NONE: Self = Self {
        tokens: TokenTotals::NONE,
        cost: MicroUsd(0),
        certainty: Certainty::Known,
    };

    /// `NONE`, as a serde default for ledgers written before spend was counted.
    pub fn none() -> Self {
        Self::NONE
    }
}

/// What one model turn tells the router about itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TurnReport {
    /// The model's usage and what it cost at the account's prices.
    Reported {
        /// Tokens the turn used.
        tokens: TokenTotals,
        /// What they cost.
        cost: MicroUsd,
    },
    /// The model reported no usage. `worst` is the most the turn could have cost, when known
    /// (its token limits at the account's prices).
    Missing {
        /// The most the turn could have cost.
        worst: Option<MicroUsd>,
    },
}

/// The ledger after one model turn. Always recorded, even if it takes the session over its
/// cap: the money is spent. A missing report is charged its worst case; with no worst case the
/// ledger becomes `Uncertain`, which `charge` and `check_turn` refuse.
pub fn charge_turn(ledger: &Ledger, report: &TurnReport) -> Ledger {
    let before = ledger.spent;
    let spent = match *report {
        TurnReport::Reported { tokens, cost } => Spent {
            tokens: before.tokens.plus(tokens),
            cost: MicroUsd(before.cost.0.saturating_add(cost.0)),
            certainty: before.certainty,
        },
        TurnReport::Missing { worst: Some(worst) } => Spent {
            cost: MicroUsd(before.cost.0.saturating_add(worst.0)),
            ..before
        },
        TurnReport::Missing { worst: None } => Spent {
            certainty: Certainty::Uncertain,
            ..before
        },
    };
    Ledger { spent, ..*ledger }
}

/// Whether the session may spend any more: out when the money counted passes the cap, or when
/// it cannot be counted.
pub fn check_turn(ledger: &Ledger, budget: &Budget) -> Result<(), BudgetKind> {
    let over = ledger.spent.cost > budget.spend;
    match (over, ledger.spent.certainty) {
        (false, Certainty::Known) => Ok(()),
        _ => Err(BudgetKind::Spend),
    }
}

/// A task's model spend, as the roster shows it: typed numbers for the shell to word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskTotals {
    /// Tokens used, by kind.
    pub tokens: TokenTotals,
    /// Money used, in millionths of a US dollar.
    pub cost: MicroUsd,
    /// Money left under the session's spend cap (zero when it is out).
    pub remaining: MicroUsd,
    /// Whether `cost` is the whole bill.
    pub certainty: Certainty,
}

/// The totals of a session's ledger under its budget.
pub fn totals_of(ledger: &Ledger, budget: &Budget) -> TaskTotals {
    TaskTotals {
        tokens: ledger.spent.tokens,
        cost: ledger.spent.cost,
        remaining: MicroUsd(budget.spend.0.saturating_sub(ledger.spent.cost.0)),
        certainty: ledger.spent.certainty,
    }
}
