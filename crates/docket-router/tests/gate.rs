//! The order in which a call is refused, reviewed, asked or let through.

use action_review::RepeatState;
use docket_core::*;
use docket_router::*;
use porter_core::consent::{GrantScope, Verdict};
use porter_core::{Count, GrantId};
use prov::{Effect, SpaceId, SpaceScope, UnixSeconds};
use std::collections::BTreeMap;

struct World {
    halt: KillSwitch,
    ledger: Ledger,
    budget: Budget,
    cost: Cost,
}

fn world() -> World {
    World {
        halt: KillSwitch {
            all: Halt::Running,
            spaces: BTreeMap::new(),
        },
        ledger: Ledger::new(UnixSeconds(0)),
        budget: AgentConfig::default().budget,
        cost: Cost {
            effect: Effect::UndoableWrite,
            entities: Count(1),
            depth: Depth(0),
            review: Reviewed::No,
        },
    }
}

fn granted() -> Verdict {
    Verdict::Granted {
        grant: GrantId::parse("g-1").expect("grant"),
        scope: GrantScope::Always,
    }
}

fn decide(w: &World, consent: &Verdict, ruling: &Ruling, repeat: RepeatState) -> Pending {
    let space = SpaceId::parse("work").expect("space");
    gate(&GateInputs {
        halt: &w.halt,
        space: &space,
        ledger: &w.ledger,
        budget: &w.budget,
        cost: &w.cost,
        now: UnixSeconds(1),
        consent,
        ruling,
        impact: Impact::Low,
        repeat,
    })
}

#[test]
fn the_gate_decides_in_a_fixed_order() {
    let judged = Ruling::AllowJudged(vec![]);
    let fin = Ruling::AllowFinal(vec![]);
    let halted = {
        let mut w = world();
        w.halt.all = Halt::Halted {
            since: UnixSeconds(0),
            by: HaltCause::KillChord,
        };
        w
    };
    let spent = {
        let mut w = world();
        w.ledger.calls = w.budget.calls;
        w
    };
    let cases: Vec<(&str, Pending, Pending)> = vec![
        (
            "final and granted runs",
            decide(&world(), &granted(), &fin, RepeatState::Fresh),
            Pending::Run,
        ),
        (
            "judged goes to the quick stage",
            decide(&world(), &granted(), &judged, RepeatState::Fresh),
            Pending::NeedsReview(vec![Stage::Quick]),
        ),
        (
            "ask confirms with its reasons",
            decide(
                &world(),
                &granted(),
                &Ruling::Ask(vec![AskReason::Tainted]),
                RepeatState::Fresh,
            ),
            Pending::Confirm(vec![AskReason::Tainted]),
        ),
        (
            "deny refuses",
            decide(
                &world(),
                &granted(),
                &Ruling::Deny(vec![]),
                RepeatState::Fresh,
            ),
            Pending::Refuse(CallRefusal::Denied(DenyCode::NotAllowed)),
        ),
        (
            "a first use asks even when policy allows",
            decide(&world(), &Verdict::Ask, &fin, RepeatState::Fresh),
            Pending::Confirm(vec![AskReason::FirstUse]),
        ),
        (
            "a first use adds its reason to an ask",
            decide(
                &world(),
                &Verdict::Ask,
                &Ruling::Ask(vec![AskReason::Tainted]),
                RepeatState::Fresh,
            ),
            Pending::Confirm(vec![AskReason::Tainted, AskReason::FirstUse]),
        ),
        (
            "a denied consent refuses even a final allow",
            decide(&world(), &Verdict::Denied, &fin, RepeatState::Fresh),
            Pending::Refuse(CallRefusal::Denied(DenyCode::NotAllowed)),
        ),
        (
            "an exact repeat is refused before consent or policy",
            decide(&world(), &granted(), &fin, RepeatState::Repeated),
            Pending::Refuse(CallRefusal::Denied(DenyCode::Repeated)),
        ),
        (
            "a halt wins over everything",
            decide(&halted, &granted(), &fin, RepeatState::Repeated),
            Pending::Refuse(CallRefusal::Halted(SpaceScope::Any)),
        ),
        (
            "a budget wins over an allow",
            decide(&spent, &granted(), &fin, RepeatState::Fresh),
            Pending::Refuse(CallRefusal::OverBudget(BudgetKind::Calls)),
        ),
    ];
    for (name, got, want) in cases {
        assert_eq!(got, want, "case: {name}");
    }
}

#[test]
fn a_halt_of_one_space_names_that_space() {
    let mut w = world();
    let work = SpaceId::parse("work").expect("space");
    w.halt.spaces.insert(
        work.clone(),
        Halt::Halted {
            since: UnixSeconds(0),
            by: HaltCause::StopKey,
        },
    );
    let got = decide(
        &w,
        &granted(),
        &Ruling::AllowFinal(vec![]),
        RepeatState::Fresh,
    );
    assert_eq!(
        got,
        Pending::Refuse(CallRefusal::Halted(SpaceScope::Only(work)))
    );
}
