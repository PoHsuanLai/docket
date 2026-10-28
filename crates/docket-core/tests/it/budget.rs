//! Budgets and the kill switch.

use crate::support::*;
use docket_core::*;
use docket_vocab::Depth;
use porter_core::MicroUsd;
use porter_ids::{Count, Tokens};
use prov::Effect;
use std::collections::BTreeMap;

fn budget() -> Budget {
    AgentConfig::default().budget
}

fn cost(effect: Effect, entities: u32, depth: u8, reviewed: bool) -> Cost {
    Cost {
        effect,
        entities: Count(entities),
        depth: Depth(depth),
        reviewed,
    }
}

#[test]
fn budget_charge_table() {
    let start = at(1_000);
    let fresh = Ledger::new(start);
    let tight = Budget {
        calls: Count(2),
        writes: Count(1),
        outbound: Count(1),
        destructive: Count(1),
        per_minute: Count(2),
        reviews: Count(1),
        spend: MicroUsd(0),
        ..budget()
    };
    let paid = Budget {
        spend: MicroUsd(10),
        ..budget()
    };
    let used = |f: &dyn Fn(&mut Ledger)| {
        let mut l = fresh;
        f(&mut l);
        l
    };
    type Case = (
        &'static str,
        Ledger,
        Budget,
        Cost,
        i64,
        Result<(), BudgetKind>,
    );
    let cases: Vec<Case> = vec![
        (
            "a read is within every budget",
            fresh,
            budget(),
            cost(Effect::Read, 1, 0, false),
            1_001,
            Ok(()),
        ),
        (
            "calls run out",
            used(&|l| l.calls = Count(2)),
            tight,
            cost(Effect::Read, 1, 0, false),
            1_001,
            Err(BudgetKind::Calls),
        ),
        (
            "a write counts against writes",
            used(&|l| l.writes = Count(1)),
            tight,
            cost(Effect::UndoableWrite, 1, 0, false),
            1_001,
            Err(BudgetKind::Writes),
        ),
        (
            "a read ignores the write budget",
            used(&|l| l.writes = Count(1)),
            tight,
            cost(Effect::Read, 1, 0, false),
            1_001,
            Ok(()),
        ),
        (
            "outbound runs out",
            used(&|l| l.outbound = Count(1)),
            tight,
            cost(Effect::Outbound, 1, 0, false),
            1_001,
            Err(BudgetKind::Outbound),
        ),
        (
            "destructive runs out",
            used(&|l| l.destructive = Count(1)),
            tight,
            cost(Effect::Destructive, 1, 0, false),
            1_001,
            Err(BudgetKind::Destructive),
        ),
        (
            "too many things in one call",
            fresh,
            budget(),
            cost(Effect::Read, 51, 0, false),
            1_001,
            Err(BudgetKind::FanOut),
        ),
        (
            "a chain too deep",
            fresh,
            budget(),
            cost(Effect::Read, 1, 5, false),
            1_001,
            Err(BudgetKind::Chain),
        ),
        (
            "wall time",
            fresh,
            budget(),
            cost(Effect::Read, 1, 0, false),
            1_000 + 1801,
            Err(BudgetKind::Wall),
        ),
        (
            "the rate window",
            used(&|l| l.in_window = Count(2)),
            tight,
            cost(Effect::Read, 1, 0, false),
            1_010,
            Err(BudgetKind::Rate),
        ),
        (
            "a new window forgets the rate",
            used(&|l| l.in_window = Count(2)),
            tight,
            cost(Effect::Read, 1, 0, false),
            1_061,
            Ok(()),
        ),
        (
            "spend at the cap still has room",
            used(&|l| l.spent.cost = MicroUsd(10)),
            paid,
            cost(Effect::Read, 1, 0, false),
            1_001,
            Ok(()),
        ),
        (
            "spend past the cap runs out",
            used(&|l| l.spent.cost = MicroUsd(11)),
            paid,
            cost(Effect::Read, 1, 0, false),
            1_001,
            Err(BudgetKind::Spend),
        ),
        (
            "spend that cannot be counted runs out",
            used(&|l| l.spent.certainty = Certainty::Uncertain),
            paid,
            cost(Effect::Read, 1, 0, false),
            1_001,
            Err(BudgetKind::Spend),
        ),
        (
            "reviews run out",
            used(&|l| l.reviews = Count(1)),
            tight,
            cost(Effect::Read, 1, 0, true),
            1_001,
            Err(BudgetKind::Reviews),
        ),
    ];
    for (name, ledger, budget, cost, now, want) in cases {
        let got = charge(&ledger, &budget, &cost, at(now));
        assert_eq!(
            got.as_ref().map(|_| ()).map_err(|k| *k),
            want,
            "case: {name}"
        );
    }
}

#[test]
fn turn_charge_table() {
    let tokens = |i, o, c| TokenTotals {
        input: Tokens(i),
        output: Tokens(o),
        cached: Tokens(c),
    };
    let paid = Budget {
        spend: MicroUsd(100),
        ..budget()
    };
    // (name, report, tokens after, cost after, certainty after, within the cap)
    let cases = [
        (
            "a reported turn is counted by kind",
            TurnReport::Reported {
                tokens: tokens(10, 5, 4),
                cost: MicroUsd(60),
            },
            tokens(10, 5, 4),
            60,
            Certainty::Known,
            true,
        ),
        (
            "a turn that costs more than the cap is recorded and stops the next",
            TurnReport::Reported {
                tokens: tokens(1, 1, 0),
                cost: MicroUsd(101),
            },
            tokens(1, 1, 0),
            101,
            Certainty::Known,
            false,
        ),
        (
            "a missing report is charged its worst case",
            TurnReport::Missing {
                worst: Some(MicroUsd(70)),
            },
            TokenTotals::NONE,
            70,
            Certainty::Known,
            true,
        ),
        (
            "a worst case over the cap stops the next turn",
            TurnReport::Missing {
                worst: Some(MicroUsd(200)),
            },
            TokenTotals::NONE,
            200,
            Certainty::Known,
            false,
        ),
        (
            "a missing report with no worst case makes the cap uncertain",
            TurnReport::Missing { worst: None },
            TokenTotals::NONE,
            0,
            Certainty::Uncertain,
            false,
        ),
    ];
    for (name, report, want_tokens, want_cost, want_certainty, within) in cases {
        let after = charge_turn(&Ledger::new(at(0)), &report);
        assert_eq!(after.spent.tokens, want_tokens, "tokens: {name}");
        assert_eq!(after.spent.cost, MicroUsd(want_cost), "cost: {name}");
        assert_eq!(after.spent.certainty, want_certainty, "certainty: {name}");
        assert_eq!(check_turn(&after, &paid).is_ok(), within, "cap: {name}");
    }
    let twice = charge_turn(
        &charge_turn(
            &Ledger::new(at(0)),
            &TurnReport::Reported {
                tokens: tokens(u32::MAX, 0, 0),
                cost: MicroUsd(u64::MAX),
            },
        ),
        &TurnReport::Reported {
            tokens: tokens(1, 1, 1),
            cost: MicroUsd(1),
        },
    );
    assert_eq!(twice.spent.tokens.input, Tokens(u32::MAX), "saturates");
    assert_eq!(twice.spent.cost, MicroUsd(u64::MAX), "saturates");
}

#[test]
fn an_old_ledger_without_spend_reads_as_nothing_spent() {
    let mut json = serde_json::to_value(Ledger::new(at(3))).expect("json");
    json.as_object_mut().expect("object").remove("spent");
    let back: Ledger = serde_json::from_value(json).expect("old ledger");
    assert_eq!(back, Ledger::new(at(3)));
}

#[test]
fn charge_counts_exactly_what_the_call_used() {
    let before = Ledger::new(at(0));
    let after = charge(
        &before,
        &budget(),
        &cost(Effect::Outbound, 3, 1, true),
        at(5),
    )
    .expect("within budget");
    assert_eq!(after.calls, Count(before.calls.0 + 1));
    assert_eq!(after.writes, Count(before.writes.0 + 1));
    assert_eq!(after.outbound, Count(before.outbound.0 + 1));
    assert_eq!(after.destructive, before.destructive);
    assert_eq!(after.reviews, Count(before.reviews.0 + 1));
    assert_eq!(after.in_window, Count(before.in_window.0 + 1));
}

#[test]
fn halt_global_overrides_space() {
    let work = space("work");
    let halted_at = |by| Halt::Halted { since: at(1), by };
    let cases = [
        ("running", Halt::Running, Halt::Running, None),
        (
            "space only",
            Halt::Running,
            halted_at(HaltCause::StopKey),
            Some(HaltCause::StopKey),
        ),
        (
            "global only",
            halted_at(HaltCause::KillChord),
            Halt::Running,
            Some(HaltCause::KillChord),
        ),
        (
            "global wins",
            halted_at(HaltCause::KillChord),
            halted_at(HaltCause::StopKey),
            Some(HaltCause::KillChord),
        ),
    ];
    for (name, all, in_work, want) in cases {
        let switch = KillSwitch {
            all,
            spaces: BTreeMap::from([(work.clone(), in_work)]),
            audit_lost: Count(0),
        };
        assert_eq!(halted(&switch, &work), want, "case: {name}");
        assert_eq!(
            halted(&switch, &space("home")),
            match all {
                Halt::Running => None,
                Halt::Halted { by, .. } => Some(by),
            },
            "other Space, case: {name}"
        );
    }
}
