//! `checkpoint_rows`: the restore-point notes of a session's log, as the list shows them.

use crate::codec::{Logged, Read};
use crate::entry::SessionEntry;
use docket_core::{CheckpointEvent, CheckpointNote, CheckpointRow, SavedState};

/// The rows of a session's list, oldest first, from its log. Every saved point is `Available`
/// here: what the store still holds is the router's to fill in. A restore is two rows, the safety
/// point it saved first (a point like any other, so the restore can be undone) and the restore
/// itself. Rows that cannot be read, legacy records and other entries add nothing.
pub fn checkpoint_rows(rows: &[Logged]) -> Vec<CheckpointRow> {
    rows.iter()
        .filter_map(|row| match &row.read {
            Read::Entry(entry) => match entry.as_ref() {
                SessionEntry::Checkpoint(note) => Some(note),
                _ => None,
            },
            Read::Legacy(_) | Read::Unreadable(_) => None,
        })
        .flat_map(note_row)
        .collect()
}

/// The rows of a list from the notes themselves, oldest first: `checkpoint_rows` for a holder
/// that kept the notes it wrote (the router keeps each live session's).
pub fn note_rows(notes: &[CheckpointNote]) -> Vec<CheckpointRow> {
    notes.iter().flat_map(note_row).collect()
}

fn note_row(note: &CheckpointNote) -> Vec<CheckpointRow> {
    let saved = |id| CheckpointRow::Saved {
        id,
        turn: note.turn,
        at: note.at,
        state: SavedState::Available,
    };
    match &note.event {
        CheckpointEvent::Taken(id) => vec![saved(*id)],
        CheckpointEvent::Skipped(why) => vec![CheckpointRow::NotSaved {
            turn: note.turn,
            at: note.at,
            why: *why,
        }],
        CheckpointEvent::Restored { to, safety } => vec![
            saved(*safety),
            CheckpointRow::RestoredTo {
                id: *to,
                at: note.at,
            },
        ],
        // A kind of event a newer build wrote: no row rather than a wrong one.
        _ => Vec::new(),
    }
}
