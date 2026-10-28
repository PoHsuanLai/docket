//! The router's trace spans, named by the OpenTelemetry GenAI conventions (`genai-names`): one
//! `execute_tool` span for each action call and one `invoke_agent` span for each model turn.
//!
//! A span carries ids, action names, the effect class, the ruling, token counts and the reported charge, and
//! nothing a person wrote or an app returned: every builder takes typed values, never text of a
//! prompt, an argument or a result, so there is nothing to leak. `tracing` needs field names
//! as literals; they are the `genai_names::attr` keys (a `docket.` key where the conventions
//! have none, as for the charge).

use crate::prepared::Prepared;
use docket_core::TurnReport;
use docket_permissions_core::Ruling;
use genai_names::Operation;
use model_provider::Money;
use prov::{SessionId, TaskId};
use tracing::Span;

/// How a ruling reads in a span.
fn ruling_word(ruling: &Ruling) -> &'static str {
    match ruling {
        Ruling::Deny(_) => "deny",
        Ruling::Ask(_) => "ask",
        Ruling::AllowJudged(_) => "allow_judged",
        Ruling::AllowFinal(_) => "allow_final",
    }
}

/// The span of one action call: which action, its call id and session, its effect class and
/// the ruling the policy gave it.
pub(crate) fn execute_tool(p: &Prepared) -> Span {
    let action = format!("{}.{}", p.request.action.app, p.request.action.name);
    let session = p
        .who
        .session
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    tracing::info_span!(
        "execute_tool",
        gen_ai.operation.name = Operation::ExecuteTool.as_str(),
        gen_ai.tool.name = %action,
        gen_ai.tool.call.id = p.id.0,
        gen_ai.conversation.id = %session,
        docket.effect = p.decl.effect.slug(),
        docket.ruling = ruling_word(&p.ruling),
    )
}

/// A charge as the span writes it: the amount and the ISO code, as in `0.0042 USD`.
fn charge_text(charge: Money) -> String {
    format!("{} {}", charge.amount, charge.currency.code())
}

/// The span of one model turn of a task, written when its usage is reported: the tokens it
/// used and the charge the provider reported, when it reported one.
pub(crate) fn invoke_agent(session: &SessionId, task: &TaskId, report: &TurnReport) -> Span {
    let tokens = report.tokens;
    let charge = report.charge.map(charge_text);
    tracing::info_span!(
        "invoke_agent",
        gen_ai.operation.name = Operation::InvokeAgent.as_str(),
        gen_ai.conversation.id = %session,
        gen_ai.usage.input_tokens = tokens.input.0,
        gen_ai.usage.output_tokens = tokens.output.0,
        docket.task = %task,
        docket.cached_tokens = tokens.cached.0,
        docket.charge = charge.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_core::TokenTotals;
    use porter_ids::Tokens;
    use rust_decimal::Decimal;
    use std::sync::{Arc, Mutex};
    use tracing::span::{Attributes, Id, Record};
    use tracing::{Event, Metadata, Subscriber};

    /// Collects the field names and the operation of every span made while it is current.
    #[derive(Default)]
    struct Names(Mutex<Vec<String>>);

    impl Subscriber for Names {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, span: &Attributes<'_>) -> Id {
            let mut names = self.0.lock().expect("lock");
            names.extend(span.metadata().fields().iter().map(|f| f.name().to_owned()));
            Id::from_u64(1)
        }
        fn record(&self, _: &Id, _: &Record<'_>) {}
        fn record_follows_from(&self, _: &Id, _: &Id) {}
        fn event(&self, _: &Event<'_>) {}
        fn enter(&self, _: &Id) {}
        fn exit(&self, _: &Id) {}
    }

    #[test]
    fn a_charge_is_written_as_an_amount_and_a_currency_code() {
        assert_eq!(charge_text(Money::usd(Decimal::new(42, 4))), "0.0042 USD");
    }

    #[test]
    fn a_turn_span_has_only_names_counts_and_ids() {
        let seen = Arc::new(Names::default());
        let report = TurnReport {
            tokens: TokenTotals {
                input: Tokens(3),
                output: Tokens(2),
                cached: Tokens(1),
            },
            charge: Some(Money::usd(Decimal::new(42, 4))),
        };
        let session = SessionId::parse("s-1").expect("session");
        let task = TaskId::parse("t-1").expect("task");
        tracing::subscriber::with_default(Arc::clone(&seen), || {
            drop(invoke_agent(&session, &task, &report));
        });
        let names = seen.0.lock().expect("lock").clone();
        assert!(!names.is_empty(), "the span was made");
        for name in &names {
            let known =
                genai_names::attr::ALL.contains(&name.as_str()) || name.starts_with("docket.");
            assert!(known, "{name} is neither a GenAI name nor a docket count");
            assert!(
                !name.contains("message") && !name.contains("content"),
                "{name}"
            );
        }
    }
}
