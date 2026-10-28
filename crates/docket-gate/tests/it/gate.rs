//! How a ruling becomes the next step: refused (worded from the facts), reviewed, asked or let
//! through. Cedar decides; the gate only maps.

use docket_core::*;
use docket_gate::facts::{budget_fact, run_facts};
use docket_gate::gate::{GateInputs, Pending, gate};
use docket_permissions_core::{AskReason, DenyCode, Impact, Limited, PolicyId, Ruling, Stage};
use docket_vocab::Depth;
use policy_point::BudgetFact;
use porter_ids::Count;
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
            audit_lost: Count(0),
        },
        ledger: Ledger::new(UnixSeconds(0)),
        budget: AgentConfig::default().budget,
        cost: Cost {
            effect: Effect::UndoableWrite,
            entities: Count(1),
            depth: Depth(0),
            reviewed: false,
        },
    }
}

fn ids(names: &[&str]) -> Vec<PolicyId> {
    names.iter().map(|n| PolicyId((*n).to_owned())).collect()
}

fn denied_by(w: &World, names: &[&str]) -> Pending {
    decide(w, &Ruling::Deny(ids(names)))
}

fn decide(w: &World, ruling: &Ruling) -> Pending {
    let space = SpaceId::parse("work").expect("space");
    let now = UnixSeconds(1);
    let judged = Cost {
        reviewed: true,
        ..w.cost
    };
    let facts = run_facts(
        &w.halt,
        &space,
        &w.ledger,
        &w.budget,
        &w.cost,
        now,
        &Limited::Free,
    );
    let charged = match ruling {
        Ruling::AllowJudged(_) => budget_fact(&w.ledger, &w.budget, &judged, now),
        _ => facts.budget,
    };
    gate(&GateInputs {
        ruling,
        impact: Impact::Low,
        facts: &facts,
        charged,
    })
}

#[test]
fn the_gate_maps_a_ruling_to_the_next_step() {
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
    let reviews_spent = {
        let mut w = world();
        w.ledger.reviews = w.budget.reviews;
        w
    };
    let w = world();
    let cases: Vec<(&str, Pending, Pending)> = vec![
        (
            "final runs",
            decide(&w, &Ruling::AllowFinal(vec![])),
            Pending::Run,
        ),
        (
            "judged goes to the quick stage",
            decide(&w, &Ruling::AllowJudged(vec![])),
            Pending::NeedsReview(vec![Stage::Quick]),
        ),
        (
            "ask confirms with its reasons, as ruled",
            decide(&w, &Ruling::Ask(vec![AskReason::Tainted])),
            Pending::Confirm(vec![AskReason::Tainted]),
        ),
        (
            "deny refuses",
            denied_by(&w, &[]),
            Pending::Refuse(CallRefusal::Denied(DenyCode::NotAllowed)),
        ),
        (
            "a denied grant is not allowed",
            denied_by(&w, &["grant-denied"]),
            Pending::Refuse(CallRefusal::Denied(DenyCode::NotAllowed)),
        ),
        (
            "a repeat is worded as one",
            denied_by(&w, &["repeated"]),
            Pending::Refuse(CallRefusal::Denied(DenyCode::Repeated)),
        ),
        (
            "a limit's never is worded as one",
            denied_by(&w, &["limit-never"]),
            Pending::Refuse(CallRefusal::Denied(DenyCode::Never)),
        ),
        (
            "another rule's deny outranks a limit's never",
            denied_by(&w, &["hidden-from-agents", "limit-never"]),
            Pending::Refuse(CallRefusal::Denied(DenyCode::NotAllowed)),
        ),
        (
            "a halt is worded by its scope and wins over a repeat",
            denied_by(&halted, &["repeated", "halted"]),
            Pending::Refuse(CallRefusal::Halted(SpaceScope::Any)),
        ),
        (
            "a budget is worded by its kind",
            denied_by(&spent, &["over-budget"]),
            Pending::Refuse(CallRefusal::OverBudget(BudgetKind::Calls)),
        ),
        (
            "a review the reviews budget cannot afford is refused",
            decide(&reviews_spent, &Ruling::AllowJudged(vec![])),
            Pending::Refuse(CallRefusal::OverBudget(BudgetKind::Reviews)),
        ),
    ];
    for (name, got, want) in cases {
        assert_eq!(got, want, "case: {name}");
    }
}

#[test]
fn the_budget_fact_names_the_kind_that_is_out() {
    let w = world();
    let mut spent = w.ledger;
    spent.calls = w.budget.calls;
    assert_eq!(
        budget_fact(&w.ledger, &w.budget, &w.cost, UnixSeconds(1)),
        BudgetFact::Within
    );
    assert_eq!(
        budget_fact(&spent, &w.budget, &w.cost, UnixSeconds(1)),
        BudgetFact::Over(BudgetKind::Calls)
    );
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
    assert_eq!(
        denied_by(&w, &["halted"]),
        Pending::Refuse(CallRefusal::Halted(SpaceScope::Only(work)))
    );
}
