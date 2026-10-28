//! What a session's model turns have used: tokens by kind and the charges the providers
//! reported. Dollars come only from the reply: there is no price table and no estimate. Pure;
//! `budget::charge` reads it and `charge_turn` writes it.

use crate::budget::{Budget, BudgetKind, Ledger};
use model_provider::Money;
use porter_ids::Tokens;
use porter_infer::ChargeTotal;
use rust_decimal::Decimal;
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

/// What a session's model turns have used so far.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spent {
    /// Tokens.
    pub tokens: TokenTotals,
    /// The charges the providers reported, and how many turns reported none. Ledgers written
    /// before this field existed (they held a `cost` and a `certainty`, now ignored) read as
    /// nothing reported.
    #[serde(default)]
    pub charges: ChargeTotal,
}

impl Spent {
    /// Nothing used yet; also the serde default for ledgers written before spend was counted.
    pub fn none() -> Self {
        Self {
            tokens: TokenTotals::NONE,
            charges: ChargeTotal::default(),
        }
    }
}

/// What one model turn tells the router about itself: the tokens it used and the charge the
/// provider reported. A turn that reported no usage is `TokenTotals::NONE` and no charge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnReport {
    /// Tokens the turn used.
    pub tokens: TokenTotals,
    /// What the provider said it cost, if it said.
    pub charge: Option<Money>,
}

/// The ledger after one model turn. Always recorded, even if it takes the session over its
/// cap: the money is spent. A turn with no charge is counted as unpriced.
pub fn charge_turn(ledger: &Ledger, report: &TurnReport) -> Ledger {
    let before = ledger.spent;
    let spent = Spent {
        tokens: before.tokens.plus(report.tokens),
        charges: before.charges.counting(report.charge),
    };
    Ledger { spent, ..*ledger }
}

/// Whether the session may spend any more: out when the charges reported so far have reached
/// the cap. A cap never stops the turn in flight, only the next one; an unpriced turn, a session
/// with no cap and a cap in another currency than the reports never fail it.
pub fn check_turn(ledger: &Ledger, budget: &Budget) -> Result<(), BudgetKind> {
    let reported = ledger.spent.charges.reported;
    match budget.spend {
        Some(cap) if cap.currency == reported.currency && reported.amount >= cap.amount => {
            Err(BudgetKind::Spend)
        }
        _ => Ok(()),
    }
}

/// A task's model spend, as the roster shows it: typed numbers for the shell to word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskTotals {
    /// Tokens used, by kind.
    pub tokens: TokenTotals,
    /// The charges reported, and the turns that reported none.
    pub charges: ChargeTotal,
    /// Money left under the session's spend cap (zero when it is out); `None` when there is no
    /// cap, or the reports are in another currency than the cap.
    pub remaining: Option<Money>,
}

/// The totals of a session's ledger under its budget.
pub fn totals_of(ledger: &Ledger, budget: &Budget) -> TaskTotals {
    let charges = ledger.spent.charges;
    let remaining = budget
        .spend
        .filter(|cap| cap.currency == charges.reported.currency)
        .map(|cap| {
            let left = cap.amount.checked_sub(charges.reported.amount);
            Money::new(
                left.unwrap_or(Decimal::ZERO).max(Decimal::ZERO),
                cap.currency,
            )
        });
    TaskTotals {
        tokens: ledger.spent.tokens,
        charges,
        remaining,
    }
}
