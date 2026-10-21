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

type Edit = fn(&mut policy_point::PolicyRequest);

/// One row of the table: the name, the effect, the rows in force, what to change in the request,
/// and the cell it must reach.
type Row = (&'static str, Effect, ModeRows, Edit, Cell);

fn row(name: &'static str, effect: Effect, rows: ModeRows, edit: Edit, want: Cell) -> Row {
    (name, effect, rows, edit, want)
}

fn nothing(_: &mut policy_point::PolicyRequest) {}

fn screen_step(r: &mut policy_point::PolicyRequest) {
    r.context.grant = policy_point::GrantState::None;
    outside(r);
    untrust_planner(r);
    r.context.taint = policy_point::TaintFact::Tainted;
    allowed_run(r);
}

fn untrusted_path(r: &mut policy_point::PolicyRequest) {
    r.context.sinks.path = prov::Integrity::Untrusted;
}

fn both_legs(r: &mut policy_point::PolicyRequest) {
    r.context.saw = SessionSaw {
        private: Saw::Seen,
        untrusted: Saw::Seen,
    };
}

fn tainted(r: &mut policy_point::PolicyRequest) {
    r.context.taint = policy_point::TaintFact::Tainted;
}

fn never(r: &mut policy_point::PolicyRequest) {
    r.context.limit = policy_point::LimitFact::Never;
}

fn halted(r: &mut policy_point::PolicyRequest) {
    r.context.halt = policy_point::HaltFact::Halted;
}

#[test]
fn each_row_value_decides_its_cell_in_either_mode() {
    use Cell::{Ask, Deny, Final, Judged};
    use Effect::{Destructive, Outbound, Read, UndoableWrite};
    let auto = ModeRows::AUTO;
    let sends = ModeRows {
        send: SendRow::Runs,
        ..auto
    };
    let deletes = ModeRows {
        delete: DeleteRow::Runs,
        ..auto
    };
    let inside_asks = ModeRows {
        inside: InsideRow::Asks,
        ..auto
    };
    let other_asks = ModeRows {
        other: OtherRow::Asks,
        ..auto
    };
    let rows = [
        row("inside runs", UndoableWrite, auto, nothing, Final),
        row(
            "inside asks beats other checked",
            UndoableWrite,
            inside_asks,
            nothing,
            Ask,
        ),
        row(
            "inside asks, outside the task",
            UndoableWrite,
            inside_asks,
            outside,
            Judged,
        ),
        row(
            "other asks, outside the task",
            UndoableWrite,
            other_asks,
            outside,
            Ask,
        ),
        row(
            "other asks, inside the task",
            UndoableWrite,
            other_asks,
            nothing,
            Final,
        ),
        row("send runs", Outbound, sends, nothing, Final),
        row("send asks", Outbound, auto, nothing, Ask),
        row(
            "send runs, untrusted recipient",
            Outbound,
            sends,
            untrust_recipient,
            Ask,
        ),
        row(
            "send runs, untrusted planner",
            Outbound,
            sends,
            untrust_planner,
            Ask,
        ),
        row("send runs, Rule of Two", Outbound, sends, both_legs, Ask),
        row(
            "send runs, tainted with an always grant",
            Outbound,
            sends,
            tainted,
            Ask,
        ),
        row(
            "send runs, a limit says never",
            Outbound,
            sends,
            never,
            Deny,
        ),
        row("delete runs", Destructive, deletes, nothing, Final),
        row("delete asks", Destructive, auto, nothing, Ask),
        row(
            "delete runs, untrusted path",
            Destructive,
            deletes,
            untrusted_path,
            Ask,
        ),
        row("delete runs, halted", Destructive, deletes, halted, Deny),
        row(
            "screen once, an undoable step",
            UndoableWrite,
            auto,
            screen_step,
            Final,
        ),
        row(
            "screen each, an undoable step",
            UndoableWrite,
            ModeRows {
                screen: ScreenRow::Each,
                ..auto
            },
            screen_step,
            Ask,
        ),
        row(
            "screen once, a send step",
            Outbound,
            sends,
            screen_step,
            Ask,
        ),
        row(
            "a read, whatever the rows",
            Read,
            ModeRows::MANUAL,
            nothing,
            Final,
        ),
    ];
    let pdp = policy_point::Pdp::built_in().expect("loads");
    for (name, effect, row_set, edit, want) in rows {
        // The rows decide, not the mode: the same rows reach the same cell in both.
        for mode in [Mode::Auto, Mode::Manual] {
            let request = with(effect, |r| {
                r.context.mode = mode;
                r.context.rows = row_set;
                edit(r);
            });
            let got = pdp.decide(&request);
            assert_eq!(cell(&got), want, "{name} in {mode:?}: {got:?}");
        }
    }
}

#[test]
fn a_rule_of_the_person_may_name_the_mode() {
    use policy_point::{Layer, PolicySource, SCHEMA};
    let only_manual = r#"
        @id("manual-reads-ask")
        forbid (principal, action in [Quire::Action::"perform_unasked", Quire::Action::"perform_unjudged"], resource)
        when { context.mode == "manual" && resource.effect == "read" };
    "#;
    let sources = [
        PolicySource::core(),
        PolicySource::preset(),
        PolicySource::file(Layer::Person, "mine.cedar".into(), only_manual.to_owned()),
    ];
    let layered = policy_point::Pdp::layered(SCHEMA, &sources).expect("loads");
    assert!(layered.skipped.is_empty(), "{:?}", layered.skipped);
    for (mode, want) in [(Mode::Auto, Cell::Final), (Mode::Manual, Cell::Ask)] {
        let got = layered
            .pdp
            .decide(&with(Effect::Read, |r| in_mode(r, mode)));
        assert_eq!(cell(&got), want, "{mode:?}: {got:?}");
    }
}
