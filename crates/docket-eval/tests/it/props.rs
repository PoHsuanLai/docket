//! The readers of what a run writes and a person edits: a case file, a planner case, a cassette.
//! Damaged text is an error, never a panic; what a run writes as a cassette is lines a replay
//! engine can read.

use docket_core::{ExchangeAnswer, ExchangeCall, ExchangeMessage, ModelExchange};
use docket_eval::{Case, PlannerCase, cassette_from};
use proptest::prelude::*;

fn exchange(
    text: String,
    calls: Vec<(String, String)>,
    shape: String,
    tools: Vec<String>,
) -> ModelExchange {
    ModelExchange {
        by: "eval".into(),
        n: 1,
        tier: "fast".into(),
        class: "prompt".into(),
        shape,
        tools,
        messages: vec![ExchangeMessage {
            role: "system".into(),
            text: "You write a task policy: the least".into(),
        }],
        route: vec![],
        answer: ExchangeAnswer::Replied {
            text,
            calls: calls
                .into_iter()
                .map(|(name, args)| ExchangeCall { name, args })
                .collect(),
            stop: "end_turn".into(),
        },
        took_ms: 0,
        input_tokens: 0,
        output_tokens: 0,
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(200))]

    #[test]
    fn damaged_case_files_are_errors_not_panics(text in any::<String>()) {
        let _ = toml::from_str::<Case>(&text);
        let _ = toml::from_str::<PlannerCase>(&text);
    }

    #[test]
    fn a_cassette_is_a_header_and_one_json_entry_per_exchange(
        parts in prop::collection::vec(
            (any::<String>(), prop::collection::vec((any::<String>(), any::<String>()), 0..3),
             prop::sample::select(vec!["text", "json", "choice"]).prop_map(str::to_owned),
             prop::collection::vec("[a-z.]{1,8}", 0..2)),
            0..6,
        ),
    ) {
        let exchanges: Vec<ModelExchange> = parts
            .into_iter()
            .map(|(text, calls, shape, tools)| exchange(text, calls, shape, tools))
            .collect();
        let text = cassette_from(&exchanges, "props");
        let lines: Vec<&str> = text.lines().collect();
        prop_assert_eq!(lines.len(), exchanges.len() + 1);
        let header: serde_json::Value = serde_json::from_str(lines[0]).expect("header");
        prop_assert_eq!(&header["vocab"], 1);
        for line in &lines[1..] {
            let entry: serde_json::Value = serde_json::from_str(line).expect("an entry is JSON");
            prop_assert!(entry["when"]["tools"] == "absent" || entry["when"]["tools"] == "present");
            prop_assert!(entry["reply"]["kind"].is_string());
        }
    }
}
