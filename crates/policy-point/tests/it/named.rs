//! The other named rules of the table of actions.md section 4.4, the launcher and Mcp rows, and
//! the single-query operations.

use crate::support::*;
use docket_core::*;
use policy_point::*;
use porter_core::Count;
use prov::{ActorKind, Effect, Integrity};

#[test]
fn cells_that_ask_without_a_rule_say_why() {
    let rows = [
        (
            "outside the task policy",
            with(Effect::Outbound, outside),
            AskReason::OutsideTask,
        ),
        (
            "untrusted planner",
            with(Effect::Outbound, untrust_planner),
            AskReason::Tainted,
        ),
        (
            "the effect itself",
            with(Effect::Destructive, |_| {}),
            AskReason::Effect(Effect::Destructive),
        ),
    ];
    for (name, request, want) in rows {
        let got = pdp().decide(&request);
        assert_eq!(asks(&got), [want], "{name}: {got:?}");
    }
}

#[test]
fn the_named_rules_of_the_table_ask_or_deny() {
    let rows: Vec<(&str, PolicyRequest, Cell, Option<AskReason>)> = vec![
        (
            "other Space",
            with(Effect::Read, |r| {
                r.context.target_space = SpaceRelation::Other
            }),
            Cell::Ask,
            Some(AskReason::CrossSpace),
        ),
        (
            "unbound Space reads freely",
            with(Effect::Read, |r| {
                r.context.target_space = SpaceRelation::Unbound
            }),
            Cell::Final,
            None,
        ),
        (
            "mass",
            with(Effect::UndoableWrite, |r| r.context.count = Count(21)),
            Cell::Ask,
            Some(AskReason::Mass(Count(21))),
        ),
        (
            "at the mass threshold",
            with(Effect::UndoableWrite, |r| r.context.count = Count(20)),
            Cell::Final,
            None,
        ),
        (
            "a read of many things",
            with(Effect::Read, |r| r.context.count = Count(500)),
            Cell::Final,
            None,
        ),
        (
            "ask always",
            with(Effect::Read, |r| r.resource.reach = AgentReach::AskAlways),
            Cell::Ask,
            Some(AskReason::AskAlways),
        ),
        (
            "first use",
            with(Effect::Read, |r| r.context.grant = GrantState::None),
            Cell::Ask,
            Some(AskReason::FirstUse),
        ),
        (
            "grant once",
            with(Effect::Read, |r| r.context.grant = GrantState::Once),
            Cell::Ask,
            Some(AskReason::FirstUse),
        ),
        (
            "lasting from untrusted",
            with(Effect::UndoableWrite, |r| {
                r.resource.lasting = Lasting::AgentMemory;
                r.context.args = Integrity::Untrusted;
            }),
            Cell::Ask,
            Some(AskReason::LastingFromUntrusted),
        ),
        (
            "lasting from trusted",
            with(Effect::UndoableWrite, |r| {
                r.resource.lasting = Lasting::AgentMemory
            }),
            Cell::Final,
            None,
        ),
        (
            "hidden from an agent",
            with(Effect::Read, |r| r.resource.reach = AgentReach::Hidden),
            Cell::Deny,
            None,
        ),
        (
            "grant denied",
            with(Effect::Read, |r| r.context.grant = GrantState::Denied),
            Cell::Deny,
            None,
        ),
        (
            "an MCP client restoring a workspace",
            with(Effect::Destructive, |r| {
                r.principal.kind = ActorKind::Mcp;
                r.resource.app = porter_core::AppName::parse("org.quire.Checkpoints").expect("app");
            }),
            Cell::Deny,
            None,
        ),
        (
            "an external agent restoring a workspace",
            with(Effect::Destructive, |r| {
                r.principal.kind = ActorKind::Acp;
                r.resource.app = porter_core::AppName::parse("org.quire.Checkpoints").expect("app");
            }),
            Cell::Deny,
            None,
        ),
    ];
    for (name, request, want, reason) in rows {
        let got = pdp().decide(&request);
        assert_eq!(cell(&got), want, "row {name}: {got:?}");
        if let Some(reason) = reason {
            assert!(asks(&got).contains(&reason), "row {name}: {got:?}");
        }
    }
}

#[test]
fn a_deny_names_the_forbid() {
    let got = pdp().decide(&with(Effect::Read, |r| {
        r.resource.reach = AgentReach::Hidden
    }));
    assert_eq!(
        got,
        Ruling::Deny(vec![PolicyId("hidden-from-agents".into())])
    );
}

#[test]
fn the_person_through_the_launcher_skips_policy() {
    let from_launcher = |effect, edit: fn(&mut PolicyRequest)| {
        with(effect, |r| {
            r.principal.kind = ActorKind::User;
            r.context.origin = Origin::Launcher;
            r.context.coverage = CoverageState::Outside;
            r.context.grant = GrantState::None;
            r.resource.reach = AgentReach::Hidden;
            edit(r);
        })
    };
    for effect in [Effect::UndoableWrite, Effect::Outbound, Effect::Destructive] {
        let got = pdp().decide(&from_launcher(effect, |_| {}));
        assert_eq!(cell(&got), Cell::Final, "{effect:?}: {got:?}");
    }
    // A hidden action is still the person's; a model standing in for the person is not.
    let impostor = with(Effect::Read, |r| {
        r.principal.kind = ActorKind::Companion;
        r.context.origin = Origin::Launcher;
        r.resource.reach = AgentReach::Hidden;
    });
    assert_eq!(cell(&pdp().decide(&impostor)), Cell::Deny);
}

#[test]
fn an_mcp_client_reads_and_otherwise_asks() {
    let mcp = |effect, edit: fn(&mut PolicyRequest)| {
        with(effect, |r| {
            r.principal.kind = ActorKind::Mcp;
            r.context.origin = Origin::Mcp;
            edit(r);
        })
    };
    let pdp = pdp();
    assert_eq!(cell(&pdp.decide(&mcp(Effect::Read, |_| {}))), Cell::Final);
    assert_eq!(
        cell(&pdp.decide(&mcp(Effect::UndoableWrite, |_| {}))),
        Cell::Ask
    );
    let other = mcp(Effect::Read, |r| {
        r.context.target_space = SpaceRelation::Other
    });
    assert_eq!(
        cell(&pdp.decide(&other)),
        Cell::Deny,
        "other Space is a refusal for Mcp"
    );
}

#[test]
fn other_operations_are_one_query() {
    let pdp = pdp();
    for op in [
        Op::Search,
        Op::Preview,
        Op::Suggest,
        Op::ReadContext,
        Op::Undo,
        Op::IndexPush,
    ] {
        let got = pdp.decide(&with(Effect::Read, |r| r.op = op));
        assert_eq!(cell(&got), Cell::Final, "{op:?}: {got:?}");
        let hidden = with(Effect::Read, |r| {
            r.op = op;
            r.resource.reach = AgentReach::Hidden;
        });
        assert_eq!(cell(&pdp.decide(&hidden)), Cell::Deny, "{op:?} hidden");
    }
}

#[test]
fn nothing_loosens_what_a_hijacked_reviewer_or_wide_policy_would() {
    // Whatever the task policy and strictness, the hard rules never reach AllowJudged or Final.
    let seen = SessionSaw {
        private: Saw::Seen,
        untrusted: Saw::Seen,
    };
    for s in [
        Strictness::AskMore,
        Strictness::Default,
        Strictness::TrustMore,
    ] {
        for effect in [Effect::Outbound, Effect::Destructive] {
            let bad_sink = pdp().decide(&with(effect, |r| {
                strictness(s)(r);
                r.context.sinks.path = Integrity::Untrusted;
            }));
            assert_eq!(cell(&bad_sink), Cell::Ask, "{s:?} {effect:?}");
        }
        let two = pdp().decide(&with(Effect::Outbound, |r| {
            strictness(s)(r);
            r.context.saw = seen;
        }));
        assert_eq!(cell(&two), Cell::Ask, "{s:?}");
    }
}
