//! The shipped schema and default policies load and validate strictly.

use docket_core::PolicyId;
use policy_point::{DEFAULT_POLICIES, Pdp, PolicyError, SCHEMA};

#[test]
fn schema_and_default_policies_validate() {
    let pdp = Pdp::standard().unwrap_or_else(|e| panic!("{e}"));
    assert!(!pdp.policy_ids().is_empty());
}

#[test]
fn the_two_named_hard_rules_are_present_by_id() {
    let ids = Pdp::standard().expect("loads").policy_ids();
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
        Pdp::load("namespace {", DEFAULT_POLICIES),
        Err(PolicyError::Schema(_))
    ));
}

#[test]
fn the_rules_apply_to_both_unasked_and_unjudged_queries() {
    // The rules forbid the two stricter queries, so no strictness and no reviewer reaches an
    // allow: the text names both action ids.
    for id in ["rule-of-two", "untrusted-sink"] {
        let at = DEFAULT_POLICIES.find(&format!("@id(\"{id}\")")).expect(id);
        let rule = &DEFAULT_POLICIES[at..];
        let end = rule.find("};").expect("end of rule");
        let rule = &rule[..end];
        assert!(
            rule.contains("perform_unasked") && rule.contains("perform_unjudged"),
            "{id}"
        );
        assert!(rule.starts_with(&format!("@id(\"{id}\")\nforbid")), "{id}");
    }
}
