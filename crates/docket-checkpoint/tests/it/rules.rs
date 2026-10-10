//! `retention`, `next_id` and `decide`, as tables.

use docket_checkpoint::{Decision, Saved, TreeId, WorkRoot, decide, next_id, retention};
use docket_core::{
    AbsPath, CheckpointEvent, CheckpointId, CheckpointNote, Days, Retention, Rewind, SkipReason,
    TurnId, Workspace,
};
use porter_core::{Count, UnixSeconds};

const DAY: i64 = 86_400;
const NOW: i64 = 100 * DAY;

/// Points `(number, age in days)`, in the order given.
fn held(points: &[(u32, i64)]) -> Vec<Saved> {
    points
        .iter()
        .map(|(id, age)| Saved {
            id: CheckpointId(*id),
            at: UnixSeconds(NOW - age * DAY),
            tree: TreeId::new(format!("t{id}")),
        })
        .collect()
}

#[test]
fn retention_drops_what_is_beyond_the_last_n_or_older_than_the_days() {
    let fresh_21: Vec<(u32, i64)> = (1..=21).map(|id| (id, 1)).collect();
    let mixed: Vec<(u32, i64)> = (1..=21)
        .map(|id| (id, if id == 5 { 20 } else { 1 }))
        .collect();
    // (name, held, last, days, dropped)
    type Held = Vec<(u32, i64)>;
    let rows: [(&str, Held, u32, u32, Vec<u32>); 9] = [
        ("the 21st point drops the first", fresh_21, 20, 14, vec![1]),
        (
            "a 15 day old point drops",
            vec![(1, 15), (2, 1)],
            20,
            14,
            vec![1],
        ),
        (
            "a 14 day old point stays",
            vec![(1, 14), (2, 0)],
            20,
            14,
            vec![],
        ),
        ("both limits at once", mixed, 20, 14, vec![1, 5]),
        ("nothing held, nothing dropped", vec![], 20, 14, vec![]),
        (
            "the order given does not matter",
            vec![(3, 0), (1, 0), (2, 0)],
            2,
            14,
            vec![1],
        ),
        (
            "a limit of none drops all",
            vec![(1, 0), (2, 0)],
            0,
            14,
            vec![1, 2],
        ),
        (
            "zero days drops all but today's",
            vec![(1, 1), (2, 0)],
            20,
            0,
            vec![1],
        ),
        ("a clock behind keeps", vec![(1, -3)], 20, 14, vec![]),
    ];
    for (name, points, last, days, want) in rows {
        let keep = Retention {
            last: Count(last),
            days: Days(days),
        };
        let got = retention(&held(&points), UnixSeconds(NOW), keep);
        let want: Vec<CheckpointId> = want.into_iter().map(CheckpointId).collect();
        assert_eq!(got, want, "{name}");
    }
}

fn note(event: CheckpointEvent) -> CheckpointNote {
    CheckpointNote {
        turn: TurnId(1),
        at: UnixSeconds(0),
        event,
    }
}

#[test]
fn the_next_number_follows_the_highest_taken_or_safety_point() {
    let taken = |n| note(CheckpointEvent::Taken(CheckpointId(n)));
    let skipped = note(CheckpointEvent::Skipped(SkipReason::NoHistory));
    let restored = |to, safety| {
        note(CheckpointEvent::Restored {
            to: CheckpointId(to),
            safety: CheckpointId(safety),
        })
    };
    let rows: [(&str, Vec<CheckpointNote>, u32); 6] = [
        ("a new session starts at 1", vec![], 1),
        ("skips take no number", vec![skipped.clone()], 1),
        ("after two points", vec![taken(1), taken(2)], 3),
        (
            "a restore's safety point counts",
            vec![taken(1), skipped, restored(1, 2)],
            3,
        ),
        (
            "numbers are never reused after a restore",
            vec![taken(3), restored(1, 5), taken(4)],
            6,
        ),
        ("the log order does not matter", vec![taken(4), taken(2)], 5),
    ];
    for (name, notes, want) in rows {
        assert_eq!(next_id(&notes), CheckpointId(want), "{name}");
    }
}

#[test]
fn a_turn_takes_a_point_unless_there_is_no_workspace_or_the_agent_keeps_its_own() {
    let work = Workspace::parse("/work/app").expect("workspace");
    let root = WorkRoot::new(AbsPath::parse("/work/app").expect("path"));
    let stepping = Workspace::parse("/work/../app").expect("workspace");
    let rows: [(&str, Rewind, Option<&Workspace>, Decision); 5] = [
        (
            "docket keeps",
            Rewind::Docket,
            Some(&work),
            Decision::Take(root),
        ),
        (
            "the agent keeps its own",
            Rewind::Agent,
            Some(&work),
            Decision::Skip(SkipReason::AgentKeepsOwn),
        ),
        (
            "no workspace, docket",
            Rewind::Docket,
            None,
            Decision::Nothing,
        ),
        (
            "no workspace, agent",
            Rewind::Agent,
            None,
            Decision::Nothing,
        ),
        (
            "a workspace that cannot be normalised",
            Rewind::Docket,
            Some(&stepping),
            Decision::Skip(SkipReason::Failed),
        ),
    ];
    for (name, rewind, cwd, want) in rows {
        assert_eq!(decide(rewind, cwd), want, "{name}");
    }
}
