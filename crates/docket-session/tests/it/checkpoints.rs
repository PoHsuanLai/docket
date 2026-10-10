//! The checkpoint entry and the tolerant reader: codec, resume, fork, export and the list fold.

use crate::support::*;
use docket_core::{
    CheckpointEvent, CheckpointId, CheckpointNote, CheckpointRow, SavedState, SkipReason, TurnId,
};
use docket_session::*;
use prov::UnixSeconds;

fn note(turn: u64, at: i64, event: CheckpointEvent) -> SessionEntry {
    SessionEntry::Checkpoint(CheckpointNote {
        turn: TurnId(turn),
        at: UnixSeconds(at),
        event,
    })
}

/// A newer build's line of a kind this one does not know, read as a row.
fn from_the_future(place: u64) -> Logged {
    decode(
        "future_kind",
        r#"{"version":1,"kind":"future_kind","v":{"anything":[1,2]}}"#,
        Seq(place),
    )
}

/// Opened, a turn, then every kind of checkpoint note and a line from the future.
fn with_notes() -> Vec<Logged> {
    let mut all = rows(&[
        SessionEntry::Opened(opening()),
        SessionEntry::Turn(turn(1, "go")),
        note(1, 10, CheckpointEvent::Taken(CheckpointId(1))),
    ]);
    all.push(from_the_future(3));
    let mut tail = rows(&[
        note(2, 20, CheckpointEvent::Skipped(SkipReason::NoHistory)),
        note(
            3,
            30,
            CheckpointEvent::Restored {
                to: CheckpointId(1),
                safety: CheckpointId(2),
            },
        ),
    ]);
    for (i, row) in tail.iter_mut().enumerate() {
        row.seq = Seq(4 + i as u64);
    }
    all.extend(tail);
    all
}

#[test]
fn a_checkpoint_entry_round_trips_and_an_unknown_kind_is_kept_not_refused() {
    let entry = note(4, 99, CheckpointEvent::Taken(CheckpointId(7)));
    let enc = encode(Seq(0), &entry).expect("encode");
    assert_eq!(enc.kind, "companion.session.checkpoint");
    assert_eq!(
        decode("checkpoint", &enc.json, Seq(9)).read,
        Read::Entry(Box::new(entry))
    );

    let Read::Entry(unknown) = from_the_future(0).read else {
        panic!("an unknown kind of this version reads as an entry");
    };
    let SessionEntry::Unknown(kept) = *unknown else {
        panic!("the entry is Unknown");
    };
    assert_eq!(kept.kind, "future_kind");
    assert_eq!(kept.v["anything"][1], 2);
    // It is never written back.
    assert!(encode(Seq(0), &SessionEntry::Unknown(kept)).is_err());
}

#[test]
fn a_resume_ignores_notes_and_unknown_lines() {
    let all = with_notes();
    let plain = resume_plan(&all[..2]).expect("plan");
    let noted = resume_plan(&all).expect("plan");
    assert_eq!(noted.standing, Standing::Open);
    assert!(noted.faults.is_empty());
    assert_eq!(noted, plain);
}

#[test]
fn a_fork_drops_notes_and_unknown_lines_and_an_export_keeps_them() {
    let all = with_notes();
    let child = fork(&session("s-1"), &all, Seq(5), task("t-2")).expect("fork");
    assert!(
        child
            .iter()
            .all(|e| !matches!(e, SessionEntry::Checkpoint(_) | SessionEntry::Unknown(_)))
    );

    let doc = export(&session("s-1"), &all).expect("export");
    assert_eq!(doc.entries.len(), all.len());
    assert!(
        doc.entries
            .iter()
            .any(|e| matches!(e, SessionEntry::Unknown(_)))
    );
    let back = from_json(&to_json(&doc).expect("json")).expect("reads back");
    assert_eq!(back, doc);
}

#[test]
fn the_list_folds_taken_skipped_and_restored_notes_in_log_order() {
    let rows = checkpoint_rows(&with_notes());
    let saved = |id, turn, at| CheckpointRow::Saved {
        id: CheckpointId(id),
        turn: TurnId(turn),
        at: UnixSeconds(at),
        state: SavedState::Available,
    };
    assert_eq!(
        rows,
        vec![
            saved(1, 1, 10),
            CheckpointRow::NotSaved {
                turn: TurnId(2),
                at: UnixSeconds(20),
                why: SkipReason::NoHistory,
            },
            // The safety point is a point like any other; then the restore itself.
            saved(2, 3, 30),
            CheckpointRow::RestoredTo {
                id: CheckpointId(1),
                at: UnixSeconds(30),
            },
        ]
    );
    assert!(checkpoint_rows(&[]).is_empty());
}
