//! The layered loader: core and preset make today's set, a person's forbid is added on top, and
//! a bad source is handled by its layer without ever loosening the set.

use crate::support::*;
use docket_core::{AskReason, PolicyId, Ruling};
use policy_point::{Layer, LayerError, Pdp, PolicyOrigin, PolicySource, SCHEMA};
use prov::Effect;
use std::path::PathBuf;

const TODAYS_IDS: [&str; 22] = [
    "attempt",
    "read-final",
    "user-launcher",
    "undoable-trusted-inside-final",
    "undoable-judged",
    "outbound-trusted-inside-judged",
    "destructive-trusted-inside-judged",
    "cli-granted-final",
    "hidden-from-agents",
    "grant-denied",
    "mcp-other-space",
    "checkpoints-not-for-agents",
    "rule-of-two",
    "untrusted-sink",
    "lasting-from-untrusted",
    "cross-space",
    "mass",
    "ask-always",
    "cli-asks",
    "cli-destructive-asks",
    "first-use",
    "mcp-asks",
];

const NO_READS: &str = r#"
    @id("no-reads")
    forbid (principal, action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"], resource)
    when { resource.effect == "read" };
"#;
const A_PERMIT: &str = r#"
    @id("let-all")
    permit (principal, action, resource);
"#;

fn file(layer: Layer, name: &str, text: &str) -> PolicySource {
    PolicySource::file(layer, PathBuf::from(name), text.to_owned())
}

fn built_in() -> Vec<PolicySource> {
    vec![PolicySource::core(), PolicySource::standard_preset()]
}

fn with_extra(extra: PolicySource) -> Vec<PolicySource> {
    let mut sources = built_in();
    sources.push(extra);
    sources
}

fn ids(pdp: &Pdp) -> Vec<String> {
    let mut ids: Vec<String> = pdp
        .policy_ids()
        .into_iter()
        .map(|PolicyId(id)| id)
        .collect();
    ids.sort();
    ids
}

#[test]
fn core_and_standard_preset_are_exactly_todays_set() {
    let mut want: Vec<String> = TODAYS_IDS.iter().map(|s| (*s).to_owned()).collect();
    want.sort();
    assert_eq!(ids(&Pdp::standard().expect("loads")), want);
}

#[test]
fn a_person_forbid_makes_a_read_ask() {
    assert_eq!(
        cell(&pdp().decide(&base(Effect::Read))),
        Cell::Final,
        "without the rule"
    );
    let layered = Pdp::layered(
        SCHEMA,
        &with_extra(file(Layer::Person, "mine.cedar", NO_READS)),
    )
    .expect("loads");
    assert!(layered.skipped.is_empty());
    assert_eq!(
        layered.pdp.decide(&base(Effect::Read)),
        Ruling::Ask(vec![AskReason::Rule(PolicyId("no-reads".to_owned()))])
    );
}

#[test]
fn a_permit_in_the_assistant_layer_is_left_out_and_in_the_admin_layer_is_an_error() {
    let assistant = Pdp::layered(
        SCHEMA,
        &with_extra(file(Layer::Assistant, "a.cedar", A_PERMIT)),
    )
    .expect("loads");
    assert_eq!(assistant.skipped.len(), 1);
    assert_eq!(assistant.skipped[0].layer, Layer::Assistant);
    assert_eq!(ids(&assistant.pdp).len(), TODAYS_IDS.len());
    let admin = Pdp::layered(
        SCHEMA,
        &with_extra(file(Layer::Admin, "/etc/a.cedar", A_PERMIT)),
    );
    assert!(matches!(admin, Err(LayerError::Admin(r)) if r.why.contains("only forbid")));
}

#[test]
fn a_bad_admin_source_is_never_skipped() {
    let admin = Pdp::layered(
        SCHEMA,
        &with_extra(file(Layer::Admin, "/etc/b.cedar", "forbid (")),
    );
    assert!(matches!(admin, Err(LayerError::Admin(_))));
}

#[test]
fn a_duplicate_id_names_both_origins() {
    let dup = r#"@id("rule-of-two") forbid (principal, action, resource);"#;
    let admin = Pdp::layered(
        SCHEMA,
        &with_extra(file(Layer::Admin, "/etc/dup.cedar", dup)),
    );
    let Err(LayerError::Admin(rejected)) = admin else {
        panic!("not an admin error");
    };
    assert!(rejected.why.contains("built-in core"), "{rejected}");
    assert!(rejected.why.contains("/etc/dup.cedar"), "{rejected}");
    // The same id twice in the person layer: the second file is left out, naming the first.
    let twice = [
        file(Layer::Person, "one.cedar", NO_READS),
        file(Layer::Person, "two.cedar", NO_READS),
    ];
    let sources: Vec<PolicySource> = built_in().into_iter().chain(twice).collect();
    let layered = Pdp::layered(SCHEMA, &sources).expect("loads");
    assert_eq!(layered.skipped.len(), 1);
    assert!(layered.skipped[0].why.contains("one.cedar"));
    assert_eq!(
        layered.skipped[0].origin,
        PolicyOrigin::File(PathBuf::from("two.cedar"))
    );
}

#[test]
fn a_bad_person_source_is_skipped_and_the_rest_loads() {
    let bad = r#"@id("x") forbid (principal, action, resource) when { context.nope == 1 };"#;
    let sources: Vec<PolicySource> = built_in()
        .into_iter()
        .chain([
            file(Layer::Person, "bad.cedar", bad),
            file(Layer::Person, "syntax.cedar", "forbid ("),
            file(Layer::Person, "good.cedar", NO_READS),
        ])
        .collect();
    let layered = Pdp::layered(SCHEMA, &sources).expect("loads");
    assert_eq!(layered.skipped.len(), 2);
    assert!(ids(&layered.pdp).contains(&"no-reads".to_owned()));
    assert!(!ids(&layered.pdp).contains(&"x".to_owned()));
}

#[test]
fn a_bad_preset_falls_back_to_the_built_in_one() {
    let sources = [
        PolicySource::core(),
        file(Layer::Preset, "grid.cedar", "permit ("),
    ];
    let layered = Pdp::layered(SCHEMA, &sources).expect("loads");
    assert_eq!(layered.skipped.len(), 1);
    assert_eq!(layered.skipped[0].layer, Layer::Preset);
    assert_eq!(ids(&layered.pdp).len(), TODAYS_IDS.len());
    // No preset at all is the built-in one, and nothing is reported.
    let none = Pdp::layered(SCHEMA, &[PolicySource::core()]).expect("loads");
    assert!(none.skipped.is_empty());
    assert_eq!(ids(&none.pdp).len(), TODAYS_IDS.len());
}

#[test]
fn a_bad_core_is_an_error() {
    let sources = [file(Layer::Core, "core.cedar", "forbid (")];
    assert!(matches!(
        Pdp::layered(SCHEMA, &sources),
        Err(LayerError::Core(_))
    ));
}
