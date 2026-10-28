//! The layered loader: core and preset make today's set, a person's forbid is added on top, and
//! a bad source is handled by its layer without ever loosening the set.

use crate::support::*;
use docket_permissions_core::{
    AskReason, Limit, LimitCaller, LimitKey, LimitLevel, LimitMaker, LimitScope, PolicyId, Ruling,
    rule_text, with_rule,
};
use docket_vocab::ActionRef;
use policy_point::{Layer, LayerError, Pdp, PolicyOrigin, PolicySource, SCHEMA};
use prov::{ActionName, Effect, SpaceScope};
use std::path::PathBuf;

const CORE_IDS: [&str; 13] = [
    "hidden-from-agents",
    "grant-denied",
    "halted",
    "over-budget",
    "repeated",
    "limit-never",
    "mcp-other-space",
    "checkpoints-not-for-agents",
    "rule-of-two",
    "untrusted-sink",
    "lasting-from-untrusted",
    "limit-ask",
    "tainted-always-asks",
];

const PRESET_IDS: [&str; 17] = [
    "attempt",
    "read-final",
    "user-launcher",
    "undoable-trusted-inside-final",
    "undoable-judged",
    "outbound-final",
    "destructive-final",
    "screen-step-final",
    "cli-granted-final",
    "cross-space",
    "mass",
    "ask-always",
    "cli-asks",
    "cli-destructive-asks",
    "screen-each-asks",
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
    vec![PolicySource::core(), PolicySource::preset()]
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

fn want(preset: &[&str]) -> Vec<String> {
    let mut want: Vec<String> = CORE_IDS
        .iter()
        .chain(preset)
        .map(|s| (*s).to_owned())
        .collect();
    want.sort();
    want
}

#[test]
fn core_and_the_preset_make_exactly_their_rules() {
    assert_eq!(ids(&Pdp::built_in().expect("loads")), want(&PRESET_IDS));
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
    assert_eq!(ids(&assistant.pdp).len(), CORE_IDS.len() + PRESET_IDS.len());
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
    assert_eq!(ids(&layered.pdp).len(), CORE_IDS.len() + PRESET_IDS.len());
    // No preset at all is the built-in one, and nothing is reported.
    let none = Pdp::layered(SCHEMA, &[PolicySource::core()]).expect("loads");
    assert!(none.skipped.is_empty());
    assert_eq!(ids(&none.pdp).len(), CORE_IDS.len() + PRESET_IDS.len());
}

#[test]
fn a_bad_preset_never_drops_what_core_holds() {
    let sources = [
        PolicySource::core(),
        file(Layer::Preset, "grid.cedar", "permit ("),
    ];
    let layered = Pdp::layered(SCHEMA, &sources).expect("loads");
    assert_eq!(layered.skipped.len(), 1);
    // The prompt-injection guard is core's: no preset, edited or built in, can drop it.
    assert!(ids(&layered.pdp).contains(&"tainted-always-asks".to_owned()));
}

#[test]
fn a_bad_core_is_an_error() {
    let sources = [file(Layer::Core, "core.cedar", "forbid (")];
    assert!(matches!(
        Pdp::layered(SCHEMA, &sources),
        Err(LayerError::Core(_))
    ));
}

fn generated(level: LimitLevel, scope: LimitScope, space: SpaceScope, by: LimitMaker) -> Limit {
    let key = LimitKey {
        caller: LimitCaller::AnyAgent,
        scope,
        space,
        from: Effect::UndoableWrite,
        level,
    };
    Limit::new(key, by, None, prov::UnixSeconds(1))
}

fn mail_send() -> LimitScope {
    LimitScope::Action(ActionRef {
        app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
        name: ActionName::parse("mail.message.send").expect("action"),
    })
}

#[test]
fn a_generated_rule_loads_in_its_layer_and_asks_or_refuses_where_its_limit_covers() {
    let work = SpaceScope::Only(prov::SpaceId::parse("work").expect("space"));
    let ask = generated(
        LimitLevel::AlwaysAsk,
        mail_send(),
        work.clone(),
        LimitMaker::Assistant,
    );
    let never = generated(LimitLevel::Never, mail_send(), work, LimitMaker::Person);
    let sources: Vec<PolicySource> = built_in()
        .into_iter()
        .chain([
            file(Layer::Assistant, "assistant.cedar", &with_rule("", &ask)),
            file(Layer::Person, "never.cedar", &with_rule("", &never)),
        ])
        .collect();
    let layered = Pdp::layered(SCHEMA, &sources).expect("loads");
    assert!(layered.skipped.is_empty(), "{:?}", layered.skipped);
    // The person's never refuses; the assistant's ask-first only asks.
    assert_eq!(
        layered.pdp.decide(&base(Effect::UndoableWrite)),
        Ruling::Deny(vec![PolicyId(never.id.as_str().to_owned())])
    );
    // Outside its Space, below its effect and from the person's own launcher, neither covers.
    let elsewhere = with(Effect::UndoableWrite, |r| {
        r.context.space = prov::SpaceId::parse("home").expect("space");
    });
    let by_person = with(Effect::UndoableWrite, |r| {
        r.principal.kind = prov::ActorKind::User
    });
    for uncovered in [elsewhere, base(Effect::Read), by_person] {
        assert_eq!(layered.pdp.decide(&uncovered), pdp().decide(&uncovered));
    }
    // The ask-first rule alone asks, in its own words, and a permit written as a rule is not one.
    let only_ask: Vec<PolicySource> = built_in()
        .into_iter()
        .chain([file(Layer::Assistant, "assistant.cedar", &rule_text(&ask))])
        .collect();
    let layered = Pdp::layered(SCHEMA, &only_ask).expect("loads");
    assert!(layered.skipped.is_empty());
    let ruling = layered.pdp.decide(&base(Effect::UndoableWrite));
    assert!(
        matches!(&ruling, Ruling::Ask(why) if why.iter().any(
            |w| matches!(w, AskReason::Said { rule, .. } if rule.0 == ask.id.as_str()))),
        "{ruling:?}"
    );
    let sneaked = rule_text(&ask).replace("forbid", "permit");
    let rejected = Pdp::layered(
        SCHEMA,
        &built_in()
            .into_iter()
            .chain([file(Layer::Assistant, "assistant.cedar", &sneaked)])
            .collect::<Vec<_>>(),
    )
    .expect("loads");
    assert_eq!(rejected.skipped.len(), 1);
}

#[test]
fn a_rules_file_that_does_not_load_still_holds_the_nevers_it_wrote() {
    let work = SpaceScope::Only(prov::SpaceId::parse("work").expect("space"));
    let never = generated(LimitLevel::Never, mail_send(), work, LimitMaker::Person);
    let asks = generated(
        LimitLevel::AlwaysAsk,
        mail_send(),
        SpaceScope::Any,
        LimitMaker::Assistant,
    );
    // A hand edit that does not parse, once above the generated rule and once below it.
    let hand_edited = format!(
        "forbid (this is not a rule\n{}\nforbid (also not",
        rule_text(&never)
    );
    // The assistant's file has a permit by hand, which its layer refuses whole.
    let permitted = format!("{A_PERMIT}\n\n{}", rule_text(&asks));
    let sources: Vec<PolicySource> = built_in()
        .into_iter()
        .chain([
            file(Layer::Person, "never.cedar", &hand_edited),
            file(Layer::Assistant, "assistant.cedar", &permitted),
        ])
        .collect();
    let layered = Pdp::layered(SCHEMA, &sources).expect("loads");
    // Both files are reported, by name.
    let named: Vec<String> = layered
        .skipped
        .iter()
        .map(|r| r.origin.to_string())
        .collect();
    assert_eq!(
        named,
        ["never.cedar", "assistant.cedar"],
        "{:?}",
        layered.skipped
    );
    // The "never" still refuses and the "ask first" still asks.
    assert!(ids(&layered.pdp).contains(&never.id.as_str().to_owned()));
    assert_eq!(
        layered.pdp.decide(&base(Effect::UndoableWrite)),
        Ruling::Deny(vec![PolicyId(never.id.as_str().to_owned())])
    );
    let elsewhere = with(Effect::UndoableWrite, |r| {
        r.context.space = prov::SpaceId::parse("home").expect("space");
    });
    assert!(
        matches!(layered.pdp.decide(&elsewhere), Ruling::Ask(why) if why.iter().any(|reason| matches!(reason, AskReason::Said { rule, .. } if rule.0 == asks.id.as_str()))),
        "the assistant's ask-first still asks"
    );
    // The hand-written permit did not load.
    assert!(!ids(&layered.pdp).contains(&"let-all".to_owned()));
    // A generated rule that is bad by itself is left out, and the good one beside it stays.
    let broken = rule_text(&never).replace("forbid (", "forbid (((");
    let both = format!("{broken}\n\n{}", rule_text(&asks));
    let sources: Vec<PolicySource> = built_in()
        .into_iter()
        .chain([file(Layer::Person, "never.cedar", &both)])
        .collect();
    let layered = Pdp::layered(SCHEMA, &sources).expect("loads");
    assert_eq!(layered.skipped.len(), 1);
    assert!(ids(&layered.pdp).contains(&asks.id.as_str().to_owned()));
    assert!(!ids(&layered.pdp).contains(&never.id.as_str().to_owned()));
}
