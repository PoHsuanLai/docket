//! The shipped schema and built-in policies load and validate strictly.

use crate::support;
use docket_core::PolicyId;
use policy_point::{CORE_POLICIES, Pdp, PolicyError, SCHEMA};

#[test]
fn schema_and_default_policies_validate() {
    let pdp = Pdp::built_in().unwrap_or_else(|e| panic!("{e}"));
    assert!(!pdp.policy_ids().is_empty());
}

#[test]
fn the_two_named_hard_rules_are_present_by_id() {
    let ids = Pdp::built_in()
        .expect("loads")
        .policy_ids();
    for want in ["rule-of-two", "untrusted-sink"] {
        assert!(
            ids.contains(&PolicyId(want.to_owned())),
            "missing {want} in {ids:?}"
        );
    }
}

#[test]
fn a_policy_naming_an_attribute_the_schema_lacks_does_not_load() {
    let bad = r#"
        @id("bad")
        forbid (principal, action == Quire::Action::"perform", resource)
        when { context.no_such_attribute == "x" };
    "#;
    assert!(matches!(
        Pdp::load(SCHEMA, bad),
        Err(PolicyError::Invalid(_))
    ));
}

#[test]
fn a_policy_that_does_not_parse_does_not_load() {
    assert!(matches!(
        Pdp::load(SCHEMA, "forbid ("),
        Err(PolicyError::Parse(_))
    ));
}

#[test]
fn a_broken_schema_does_not_load() {
    assert!(matches!(
        Pdp::load("namespace {", CORE_POLICIES),
        Err(PolicyError::Schema(_))
    ));
}

#[test]
fn the_rules_apply_to_both_unasked_and_unjudged_queries() {
    // The rules forbid the two stricter queries, so no strictness and no reviewer reaches an
    // allow: the text names both action ids.
    for id in ["rule-of-two", "untrusted-sink"] {
        let at = CORE_POLICIES.find(&format!("@id(\"{id}\")")).expect(id);
        let rule = &CORE_POLICIES[at..];
        let end = rule.find("};").expect("end of rule");
        let rule = &rule[..end];
        assert!(
            rule.contains("perform_unasked") && rule.contains("perform_unjudged"),
            "{id}"
        );
        assert!(rule.starts_with(&format!("@id(\"{id}\")\nforbid")), "{id}");
    }
}

#[test]
fn a_policy_that_errors_asks_with_the_failure_and_never_permits() {
    // `count + max` overflows for any count above zero: Cedar skips the policy and reports it.
    let erroring = r#"
        @id("attempt")
        permit (principal, action == Quire::Action::"perform", resource);
        @id("overflow")
        permit (principal, action == Quire::Action::"perform_unasked", resource)
        when { context.count + 9223372036854775807 > 0 };
    "#;
    let pdp = Pdp::load(SCHEMA, erroring).expect("validates");
    let ruled = pdp.decide_checked(&support::base(prov::Effect::Read));
    assert_eq!(
        ruled.ruling,
        docket_core::Ruling::Ask(vec![docket_core::AskReason::EvaluationFailed])
    );
    assert_eq!(
        ruled.faults,
        [policy_point::EvalFault {
            query: "perform_unasked",
            cause: policy_point::FaultCause::Policies(1),
        }]
    );
    assert!(
        support::pdp()
            .pdp
            .decide_checked(&support::base(prov::Effect::Read))
            .faults
            .is_empty()
    );
}
