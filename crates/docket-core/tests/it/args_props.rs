//! A tool call's arguments as a model might write them, read into typed values by
//! `args_from_json`: no panic, only declared arguments, every text inside its bounds and free of
//! reordering or hidden marks, every required argument present.

use crate::support::*;
use docket_core::*;
use proptest::prelude::*;
use prov::Effect;
use serde_json::{Map, Value as Json, json};

fn decl_under_test() -> ActionDecl {
    let mut d = decl(
        "mail.message.send",
        Effect::Outbound,
        UndoSupport::NotUndoable,
    );
    d.on = TargetKind::Many(kind("mail.thread"));
    d.params = vec![
        text_param("to", ArgSink::Recipient, ParamNeed::Required),
        ParamDecl {
            name: param("body"),
            label: words("Body"),
            ty: ParamType::Text {
                max: CharCount(60),
                lines: Lines::Many,
            },
            need: ParamNeed::Optional,
            sink: ArgSink::Body,
        },
        ParamDecl {
            name: param("count"),
            label: words("Count"),
            ty: ParamType::Integer { min: 0, max: 9 },
            need: ParamNeed::Optional,
            sink: ArgSink::Inert,
        },
    ];
    d
}

fn arb_json() -> impl Strategy<Value = Json> {
    let leaf = prop_oneof![
        Just(Json::Null),
        any::<bool>().prop_map(Json::Bool),
        any::<i64>().prop_map(|n| json!(n)),
        any::<f64>().prop_map(|f| json!(f)),
        any::<String>().prop_map(Json::String),
        prop::sample::select(vec![
            "a@b.c",
            "x\u{202e}y",
            "z\u{200b}w",
            "l1\nl2",
            "t\tt",
            ""
        ])
        .prop_map(|s| json!(s)),
    ];
    leaf.prop_recursive(3, 24, 4, |inner| {
        prop_oneof![
            prop::collection::vec(inner.clone(), 0..4).prop_map(Json::Array),
            prop::collection::vec(
                (
                    prop::sample::select(vec![
                        "to", "body", "count", "target", "handle", "app", "kind", "key", "admin"
                    ]),
                    inner
                ),
                0..5
            )
            .prop_map(|pairs| Json::Object(
                pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
            )),
        ]
    })
}

fn arguments() -> impl Strategy<Value = Map<String, Json>> {
    prop::collection::vec(
        (
            prop::sample::select(vec!["to", "body", "count", "target", "admin"]),
            arb_json(),
        ),
        0..5,
    )
    .prop_map(|pairs| pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    #[test]
    fn whatever_the_model_writes_the_result_is_a_typed_value_or_a_typed_fault(given in arguments()) {
        let d = decl_under_test();
        match args_from_json(&d, Some(&given), &trusted()) {
            Err(_) => {}
            Ok((_target, args)) => {
                // Only what the action declares, and every required one.
                prop_assert!(args.keys().all(|k| d.params.iter().any(|p| &p.name == k)));
                prop_assert!(args.contains_key(&param("to")));
                for (name, held) in &args {
                    match (name.as_str(), &held.value) {
                        ("to", Value::Text(t)) => {
                            prop_assert!(t.chars().count() <= 100);
                            prop_assert!(plain_text(t, &[]), "{t:?}");
                        }
                        ("body", Value::Text(t)) => {
                            prop_assert!(t.chars().count() <= 60);
                            prop_assert!(plain_text(t, &['\n', '\r', '\t']), "{t:?}");
                        }
                        ("count", Value::Integer(n)) => prop_assert!((0..=9).contains(n)),
                        (_, Value::Handle(_)) => {}
                        (name, other) => prop_assert!(false, "{name}: {other:?}"),
                    }
                }
            }
        }
    }

    #[test]
    fn an_argument_nobody_declared_is_always_refused(
        name in "[a-z]{1,8}",
        value in arb_json(),
    ) {
        prop_assume!(!["to", "body", "count", "target"].contains(&name.as_str()));
        let d = decl_under_test();
        let given: Map<String, Json> = [(name.clone(), value)].into_iter().collect();
        prop_assert_eq!(
            args_from_json(&d, Some(&given), &trusted()).err(),
            Some(ArgsFault::Unknown(name))
        );
    }

    #[test]
    fn text_with_a_reordering_or_hidden_mark_is_never_taken(
        before in "[a-z@.]{0,8}",
        mark in prop::sample::select(vec!['\u{202a}', '\u{202e}', '\u{2066}', '\u{2069}', '\u{200b}', '\u{2060}', '\u{feff}', '\u{ad}', '\u{0}', '\u{7}']),
        after in "[a-z@.]{0,8}",
    ) {
        let d = decl_under_test();
        let thread = json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" });
        let given = json!({ "to": format!("{before}{mark}{after}"), "target": [thread] });
        let given = given.as_object().cloned().unwrap_or_default();
        prop_assert!(args_from_json(&d, Some(&given), &trusted()).is_err());
    }
}

#[test]
fn the_mark_free_text_of_the_same_shape_is_taken() {
    let d = decl_under_test();
    let thread = json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": "t1" });
    let given = json!({ "to": "alice@example.test", "target": [thread] });
    let (_, args) = args_from_json(&d, given.as_object(), &trusted()).expect("taken");
    assert_eq!(
        args[&param("to")].value,
        Value::Text("alice@example.test".into())
    );
}
