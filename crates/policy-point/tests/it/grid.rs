//! The decision grid of each preset, one row per cell, and what Auto does for first use and for
//! the steps of a screen run.

use crate::support::*;
use docket_core::*;
use porter_core::AppName;
use prov::Effect;

/// `request` for `effect`, with the planner untrusted (and the recipient too for a change) when
/// `trusted` is false, and outside the task when `inside` is false.
fn cell_request(effect: Effect, trusted: bool, inside: bool) -> policy_point::PolicyRequest {
    with(effect, |r| {
        // Reads never feed a sink; "untrusted" there is the planner alone.
        if !trusted {
            untrust_planner(r);
            if effect != Effect::Read {
                untrust_recipient(r);
            }
        }
        if !inside {
            outside(r);
        }
    })
}

#[test]
fn auto_grid() {
    use Cell::{Ask, Final, Judged};
    use Effect::{Destructive, Outbound, Read, UndoableWrite};
    let pdp = pdp();
    // (name, effect, trusted?, inside?, want)
    let rows = [
        ("read T In", Read, true, true, Final),
        ("read U Out", Read, false, false, Final),
        ("undoable T In", UndoableWrite, true, true, Final),
        ("undoable T Out", UndoableWrite, true, false, Judged),
        ("undoable U In", UndoableWrite, false, true, Judged),
        ("undoable U Out", UndoableWrite, false, false, Judged),
        ("outbound T In", Outbound, true, true, Ask),
        ("outbound T Out", Outbound, true, false, Ask),
        ("outbound U In", Outbound, false, true, Ask),
        ("outbound U Out", Outbound, false, false, Ask),
        ("destructive T In", Destructive, true, true, Ask),
        ("destructive T Out", Destructive, true, false, Ask),
        ("destructive U In", Destructive, false, true, Ask),
        ("destructive U Out", Destructive, false, false, Ask),
    ];
    for (name, effect, trusted, inside, want) in rows {
        let got = pdp.decide(&cell_request(effect, trusted, inside));
        assert_eq!(cell(&got), want, "row {name}: {got:?}");
    }
}

#[test]
fn manual_grid() {
    use Cell::{Ask, Final};
    use Effect::{Destructive, Outbound, Read, UndoableWrite};
    let pdp = manual();
    let rows = [
        ("read T In", Read, true, true, Final),
        ("read U Out", Read, false, false, Final),
        ("undoable T In", UndoableWrite, true, true, Ask),
        ("undoable U Out", UndoableWrite, false, false, Ask),
        ("outbound T In", Outbound, true, true, Ask),
        ("outbound U Out", Outbound, false, false, Ask),
        ("destructive T In", Destructive, true, true, Ask),
        ("destructive U Out", Destructive, false, false, Ask),
    ];
    for (name, effect, trusted, inside, want) in rows {
        let got = pdp.decide(&cell_request(effect, trusted, inside));
        assert_eq!(cell(&got), want, "row {name}: {got:?}");
    }
}

#[test]
fn an_untrusted_planner_alone_breaks_the_trusted_cells() {
    let pdp = pdp();
    let got = pdp.decide(&with(Effect::UndoableWrite, untrust_planner));
    assert_eq!(cell(&got), Cell::Judged, "{got:?}");
    let got = pdp.decide(&with(Effect::Outbound, untrust_planner));
    assert_eq!(cell(&got), Cell::Ask, "{got:?}");
}

fn first_use_of(app: &str, effect: Effect) -> policy_point::PolicyRequest {
    with(effect, |r| {
        r.context.grant = policy_point::GrantState::None;
        if app != "org.quire.Mail" {
            r.resource.app = AppName::parse(app).expect("app");
        }
    })
}

#[test]
fn auto_asks_for_a_first_use_only_for_a_change() {
    use Cell::{Ask, Final};
    let auto = pdp();
    let manual = manual();
    // (name, app, effect, auto, manual)
    let rows = [
        (
            "a read of mail",
            "org.quire.Mail",
            Effect::Read,
            Final,
            Final,
        ),
        (
            "a task start",
            "org.quire.Companion",
            Effect::Read,
            Final,
            Final,
        ),
        ("a recall", "org.quire.Memory", Effect::Read, Final, Final),
        (
            "staging a memory",
            "org.quire.Memory",
            Effect::UndoableWrite,
            Final,
            Ask,
        ),
        (
            "a change in mail",
            "org.quire.Mail",
            Effect::UndoableWrite,
            Ask,
            Ask,
        ),
        ("a send", "org.quire.Mail", Effect::Outbound, Ask, Ask),
        (
            "deleting a memory",
            "org.quire.Memory",
            Effect::Destructive,
            Ask,
            Ask,
        ),
    ];
    for (name, app, effect, want_auto, want_manual) in rows {
        let request = first_use_of(app, effect);
        let got = auto.decide(&request);
        assert_eq!(cell(&got), want_auto, "auto {name}: {got:?}");
        let got = manual.decide(&request);
        assert_eq!(cell(&got), want_manual, "manual {name}: {got:?}");
    }
}

#[test]
fn a_step_of_an_allowed_screen_run_runs_unless_it_sends_or_deletes() {
    use Cell::{Ask, Final};
    // A screen step: nothing granted for the class, the screen is untrusted, outside any task.
    let step = |effect: Effect, allowed: bool| {
        with(effect, |r| {
            r.context.grant = policy_point::GrantState::None;
            outside(r);
            untrust_planner(r);
            r.context.taint = policy_point::TaintFact::Tainted;
            if allowed {
                allowed_run(r);
            }
        })
    };
    let rows = [
        ("undoable, allowed", Effect::UndoableWrite, true, Final, Ask),
        (
            "undoable, not asked",
            Effect::UndoableWrite,
            false,
            Ask,
            Ask,
        ),
        ("send, allowed", Effect::Outbound, true, Ask, Ask),
        ("delete, allowed", Effect::Destructive, true, Ask, Ask),
        ("read, allowed", Effect::Read, true, Final, Final),
    ];
    for (name, effect, allowed, want_auto, want_manual) in rows {
        let got = pdp().decide(&step(effect, allowed));
        assert_eq!(cell(&got), want_auto, "auto {name}: {got:?}");
        let got = manual().decide(&step(effect, allowed));
        assert_eq!(cell(&got), want_manual, "manual {name}: {got:?}");
    }
}

#[test]
fn a_rule_with_a_why_asks_with_its_own_sentence_and_one_without_is_named() {
    use policy_point::{Layer, PolicySource, SCHEMA};
    let rule = |id: &str, why: &str| {
        format!(
            r#"{why} @id("{id}") forbid (principal, action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"], resource) when {{ resource.effect == "read" }};"#
        )
    };
    let set = |text: String| {
        let sources = [
            PolicySource::core(),
            PolicySource::file(Layer::Person, "mine.cedar".into(), text),
        ];
        policy_point::Pdp::layered(SCHEMA, &sources)
            .expect("loads")
            .pdp
    };
    let said = set(rule("mine", r#"@why("Because you said so.")"#)).decide(&base(Effect::Read));
    assert_eq!(
        asks(&said),
        [AskReason::Said {
            rule: PolicyId("mine".to_owned()),
            why: "Because you said so.".to_owned()
        }]
    );
    let bare = set(rule("mine", "")).decide(&base(Effect::Read));
    assert_eq!(asks(&bare), [AskReason::Rule(PolicyId("mine".to_owned()))]);
}
