//! `parse_verdict` over what a model might write. A reference reader says, key by key, what the
//! exact allowed shapes are; the parser agrees with it on every input, so there is no input it
//! reads as an allow (or an ask, or a refusal) that the reference does not.

use action_review::parse_verdict;
use docket_core::{ReviewError, Stage, VerdictKind};
use proptest::prelude::*;

const CODES: [&str; 11] = [
    "within_request",
    "covered_by_task_policy",
    "routine",
    "outside_request",
    "scope_creep",
    "exfiltration",
    "irreversible",
    "injection_suspected",
    "uncertain",
    "reviewer_failed",
    "disagreement",
];
const ALLOWING: [&str; 3] = ["within_request", "covered_by_task_policy", "routine"];

/// One value a model might put after a key.
#[derive(Debug, Clone)]
enum Val {
    Text(String),
    Number(i64),
    Null,
}

impl Val {
    fn json(&self) -> String {
        match self {
            Val::Text(t) => serde_json::to_string(t).unwrap_or_default(),
            Val::Number(n) => n.to_string(),
            Val::Null => "null".into(),
        }
    }
}

fn key() -> impl Strategy<Value = &'static str> {
    prop::sample::select(vec!["verdict", "code", "reason", "extra"])
}

fn text() -> impl Strategy<Value = String> {
    prop_oneof![
        prop::sample::select(vec!["allow", "ask", "deny", "ALLOW", "pass", ""])
            .prop_map(str::to_owned),
        prop::sample::select(CODES.to_vec()).prop_map(str::to_owned),
        prop::sample::select(vec!["scripted", "fine", "two words"]).prop_map(str::to_owned),
        prop::sample::select(vec!["a\nb", "x\u{202e}y", "z\u{200b}w", "tab\there"])
            .prop_map(str::to_owned),
        "[a-z ]{190,210}",
        any::<String>(),
    ]
}

fn val() -> impl Strategy<Value = Val> {
    prop_oneof![
        4 => text().prop_map(Val::Text),
        1 => any::<i64>().prop_map(Val::Number),
        1 => Just(Val::Null),
    ]
}

/// What the reference says a record of these pairs is: the exact shape, once per key.
fn reference(pairs: &[(&'static str, Val)]) -> Option<VerdictKind> {
    let get = |k: &str| {
        let found: Vec<&Val> = pairs
            .iter()
            .filter(|(n, _)| *n == k)
            .map(|(_, v)| v)
            .collect();
        match found.as_slice() {
            [Val::Text(t)] => Some(t.as_str()),
            _ => None,
        }
    };
    let exact = pairs.len() == 3 && pairs.iter().all(|(k, _)| *k != "extra");
    let (verdict, code, reason) = (get("verdict")?, get("code")?, get("reason")?);
    let plain = reason
        .chars()
        .all(|c| !c.is_control() && !docket_core::reorders(c) && !docket_core::hides(c));
    if !exact || reason.chars().count() > 200 || !plain || !CODES.contains(&code) {
        return None;
    }
    match (verdict, ALLOWING.contains(&code)) {
        ("allow", true) => Some(VerdictKind::Allow),
        ("ask", false) => Some(VerdictKind::Ask),
        ("deny", false) => Some(VerdictKind::Deny),
        _ => None,
    }
}

fn object(pairs: &[(&'static str, Val)]) -> String {
    let body: Vec<String> = pairs
        .iter()
        .map(|(k, v)| format!("\"{k}\":{}", v.json()))
        .collect();
    format!("{{{}}}", body.join(","))
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(400))]

    #[test]
    fn a_record_is_read_as_the_reference_reads_it(
        pairs in prop::collection::vec((key(), val()), 0..6),
        stage in prop::sample::select(vec![Stage::Deliberate, Stage::SecondOpinion]),
    ) {
        let raw = object(&pairs);
        let got = parse_verdict(&raw, stage).ok().map(|v| v.kind());
        prop_assert_eq!(got, reference(&pairs), "{}", raw);
    }

    #[test]
    fn a_well_formed_record_inside_anything_else_is_never_an_allow(
        before in "[ -~]{1,12}",
        after in "[ -~]{1,12}",
        stage in prop::sample::select(vec![Stage::Deliberate, Stage::SecondOpinion]),
    ) {
        let good = r#"{"verdict":"allow","code":"within_request","reason":"ok"}"#;
        for raw in [format!("{before}{good}"), format!("{good}{after}"), format!("{before}{good}{after}")] {
            let trimmed = raw.trim() == good;
            let got = parse_verdict(&raw, stage);
            prop_assert_eq!(got.is_ok(), trimmed, "{}", raw);
        }
    }

    #[test]
    fn nothing_but_the_word_pass_passes_the_quick_judge(raw in any::<String>()) {
        let got = parse_verdict(&raw, Stage::Quick);
        prop_assert_eq!(matches!(got, Ok(ref v) if v.kind() == VerdictKind::Allow), raw.trim() == "pass");
        if let Ok(v) = &got {
            prop_assert!(matches!(v.kind(), VerdictKind::Allow | VerdictKind::Ask));
        }
    }

    #[test]
    fn any_text_at_any_stage_is_a_verdict_or_a_typed_error_and_never_panics(raw in any::<String>()) {
        for stage in [Stage::Quick, Stage::Deliberate, Stage::SecondOpinion] {
            match parse_verdict(&raw, stage) {
                Ok(_) | Err(ReviewError::Unparseable | ReviewError::OutOfVocabulary) => {}
                Err(other) => prop_assert!(false, "{other:?}"),
            }
        }
    }
}

#[test]
fn the_duplicate_key_that_read_as_an_allow_is_refused() {
    let raw = r#"{"verdict":"deny","verdict":"allow","code":"within_request","reason":"x"}"#;
    assert_eq!(
        parse_verdict(raw, Stage::Deliberate),
        Err(ReviewError::Unparseable)
    );
}
