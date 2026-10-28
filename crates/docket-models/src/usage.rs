//! What a model turn reports about itself, as the router's ledger counts it: tokens by kind
//! and the charge the provider reported.

use docket_core::{TokenTotals, TurnReport};
use porter_infer::TokenUsage;

/// The ledger's report for a turn that used `usage`. The charge is whatever the provider
/// reported, passed through as it came: never worked out from a price, and `None` when the
/// provider said nothing.
pub fn report_of(usage: TokenUsage) -> TurnReport {
    TurnReport {
        tokens: TokenTotals {
            input: usage.input,
            output: usage.output,
            cached: usage.cached,
        },
        charge: usage.charge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use model_provider::Money;
    use porter_ids::Tokens;
    use rust_decimal::Decimal;

    fn usage(input: u32, output: u32, cached: u32, charge: Option<Money>) -> TokenUsage {
        TokenUsage {
            input: Tokens(input),
            output: Tokens(output),
            cached: Tokens(cached),
            charge,
        }
    }

    #[test]
    fn usage_becomes_a_report_with_the_charge_as_reported() {
        let charge = Money::usd(Decimal::new(42, 4));
        let tokens = TokenTotals {
            input: Tokens(1000),
            output: Tokens(100),
            cached: Tokens(400),
        };
        let cases = [
            ("a reported charge", Some(charge)),
            ("no charge reported", None),
        ];
        for (name, charge) in cases {
            let want = TurnReport { tokens, charge };
            assert_eq!(report_of(usage(1000, 100, 400, charge)), want, "{name}");
        }
    }

    #[test]
    fn a_reply_with_no_usage_is_no_tokens_and_no_charge() {
        let want = TurnReport {
            tokens: TokenTotals::NONE,
            charge: None,
        };
        assert_eq!(report_of(usage(0, 0, 0, None)), want);
    }
}
