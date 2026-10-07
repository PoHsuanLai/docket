//! Budgets and the kill switch.

use crate::support::*;
use docket_core::*;
use porter_core::{Count, MicroUsd};
use prov::Effect;
use std::collections::BTreeMap;

fn budget() -> Budget {
    AgentConfig::default().budget
}

fn cost(effect: Effect, entities: u32, depth: u8, review: Reviewed) -> Cost {
    Cost {
        effect,
        entities: Count(entities),
        depth: Depth(depth),
        review,
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
            cost(Effect::Read, 1, 0, Reviewed::No),
            1_001,
            Ok(()),
        ),
        (
            "calls run out",
            used(&|l| l.calls = Count(2)),
            tight,
            cost(Effect::Read, 1, 0, Reviewed::No),
            1_001,
            Err(BudgetKind::Calls),
        ),
        (
            "a write counts against writes",
            used(&|l| l.writes = Count(1)),
            tight,
            cost(Effect::UndoableWrite, 1, 0, Reviewed::No),
            1_001,
            Err(BudgetKind::Writes),
        ),
        (
            "a read ignores the write budget",
            used(&|l| l.writes = Count(1)),
            tight,
            cost(Effect::Read, 1, 0, Reviewed::No),
            1_001,
            Ok(()),
        ),
        (
            "outbound runs out",
            used(&|l| l.outbound = Count(1)),
            tight,
            cost(Effect::Outbound, 1, 0, Reviewed::No),
            1_001,
            Err(BudgetKind::Outbound),
        ),
        (
            "destructive runs out",
            used(&|l| l.destructive = Count(1)),
            tight,
            cost(Effect::Destructive, 1, 0, Reviewed::No),
            1_001,
            Err(BudgetKind::Destructive),
        ),
        (
            "too many things in one call",
            fresh,
            budget(),
            cost(Effect::Read, 51, 0, Reviewed::No),
            1_001,
            Err(BudgetKind::FanOut),
        ),
        (
            "a chain too deep",
            fresh,
            budget(),
            cost(Effect::Read, 1, 5, Reviewed::No),
            1_001,
            Err(BudgetKind::Chain),
        ),
        (
            "wall time",
            fresh,
            budget(),
            cost(Effect::Read, 1, 0, Reviewed::No),
            1_000 + 1801,
            Err(BudgetKind::Wall),
        ),
        (
            "the rate window",
            used(&|l| l.in_window = Count(2)),
            tight,
            cost(Effect::Read, 1, 0, Reviewed::No),
            1_010,
            Err(BudgetKind::Rate),
        ),
        (
            "a new window forgets the rate",
            used(&|l| l.in_window = Count(2)),
            tight,
            cost(Effect::Read, 1, 0, Reviewed::No),
            1_061,
            Ok(()),
        ),
        (
            "reviews run out",
            used(&|l| l.reviews = Count(1)),
            tight,
            cost(Effect::Read, 1, 0, Reviewed::Yes),
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
fn charge_counts_exactly_what_the_call_used() {
    let before = Ledger::new(at(0));
    let after = charge(
        &before,
        &budget(),
        &cost(Effect::Outbound, 3, 1, Reviewed::Yes),
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
