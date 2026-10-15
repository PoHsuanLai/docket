//! What a model turn reports about itself, as the router's ledger counts it: tokens by kind
//! and what they cost at the account's billing.

use docket_core::{TokenTotals, TurnReport};
use porter_core::{Billing, MicroUsd};
use porter_infer::{TokenUsage, cost};

/// The ledger's report for a turn that used `usage` on an account billed as `billing`.
///
/// A reply that counts no input and no output tokens reported nothing: it is charged `worst`
/// (the most the turn could have cost, from its token limits and the account's prices), or, when
/// that is not known, `TurnReport::Missing { worst: None }`, which makes the spend cap
/// uncertain. Money is exact for a metered account, and nothing for a free or plan account.
pub fn report_of(usage: TokenUsage, billing: &Billing, worst: Option<MicroUsd>) -> TurnReport {
    if usage.input.0 == 0 && usage.output.0 == 0 {
        return TurnReport::Missing { worst };
    }
    let money = match billing {
        Billing::Metered(prices) => cost(usage, prices),
        Billing::Free | Billing::PlanBudget => MicroUsd(0),
    };
    TurnReport::Reported {
        tokens: TokenTotals {
            input: usage.input,
            output: usage.output,
            cached: usage.cached,
        },
        cost: money,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use porter_core::{PriceTable, Tokens};

    fn usage(input: u32, output: u32, cached: u32) -> TokenUsage {
        TokenUsage {
            input: Tokens(input),
            output: Tokens(output),
            cached: Tokens(cached),
        }
    }

    #[test]
    fn usage_becomes_a_report_by_billing() {
        let metered = Billing::Metered(PriceTable {
            input_per_mtok: MicroUsd(3_000_000),
            output_per_mtok: MicroUsd(15_000_000),
        });
        let tokens = TokenTotals {
            input: Tokens(1000),
            output: Tokens(100),
            cached: Tokens(400),
        };
        let at = |cost| TurnReport::Reported {
            tokens,
            cost: MicroUsd(cost),
        };
        let cases = [
            (
                "metered: input and output priced",
                metered.clone(),
                usage(1000, 100, 400),
                None,
                at(4_500),
            ),
            (
                "free costs nothing",
                Billing::Free,
                usage(1000, 100, 400),
                None,
                at(0),
            ),
            (
                "a plan costs no money",
                Billing::PlanBudget,
                usage(1000, 100, 400),
                None,
                at(0),
            ),
            (
                "no usage is charged the worst case",
                metered.clone(),
                usage(0, 0, 0),
                Some(MicroUsd(900)),
                TurnReport::Missing {
                    worst: Some(MicroUsd(900)),
                },
            ),
            (
                "no usage and no worst case is uncertain",
                metered,
                usage(0, 0, 0),
                None,
                TurnReport::Missing { worst: None },
            ),
        ];
        for (name, billing, used, worst, want) in cases {
            assert_eq!(report_of(used, &billing, worst), want, "{name}");
        }
    }
}
