//! The pure rules: which number comes next, what is dropped, whether a turn takes a point.

use crate::ids::{Saved, WorkRoot};
use docket_core::{
    CheckpointEvent, CheckpointId, CheckpointNote, Retention, Rewind, SkipReason, Workspace,
};
use porter_core::UnixSeconds;

const SECONDS_PER_DAY: i64 = 24 * 60 * 60;

/// What a turn start does about a restore point.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Decision {
    /// Save the workspace.
    Take(WorkRoot),
    /// Save nothing, and say why in the list.
    Skip(SkipReason),
    /// Nothing to say at all: the session has no workspace.
    Nothing,
}

/// Whether the turn that starts now takes a point. A session with no workspace (a plain chat)
/// writes nothing; an agent that keeps its own history is skipped, and the list says so.
pub fn decide(rewind: Rewind, cwd: Option<&Workspace>) -> Decision {
    let Some(workspace) = cwd else {
        return Decision::Nothing;
    };
    match rewind {
        Rewind::Agent => Decision::Skip(SkipReason::AgentKeepsOwn),
        Rewind::Docket => match WorkRoot::of(workspace) {
            Ok(root) => Decision::Take(root),
            Err(_) => Decision::Skip(SkipReason::Failed),
        },
    }
}

/// The number for the next point: one more than any the notes name (a taken point or the safety
/// point of a restore), 1 for a session with none. Numbers are never reused.
pub fn next_id(notes: &[CheckpointNote]) -> CheckpointId {
    let highest = notes
        .iter()
        .filter_map(|note| match &note.event {
            CheckpointEvent::Taken(id) => Some(id.0),
            CheckpointEvent::Restored { safety, .. } => Some(safety.0),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    CheckpointId(highest.saturating_add(1))
}

/// The points to drop, oldest first: any that is not among the newest `keep.last` (by number) or
/// is older than `keep.days` (by the time it was taken). Both limits apply.
pub fn retention(held: &[Saved], now: UnixSeconds, keep: Retention) -> Vec<CheckpointId> {
    let mut by_number: Vec<&Saved> = held.iter().collect();
    by_number.sort_by_key(|saved| saved.id);
    let newest_from = by_number
        .len()
        .saturating_sub(usize::try_from(keep.last.0).unwrap_or(usize::MAX));
    let longest = i64::from(keep.days.0).saturating_mul(SECONDS_PER_DAY);
    by_number
        .iter()
        .enumerate()
        .filter(|(place, saved)| {
            let too_many = *place < newest_from;
            let too_old = now.0.saturating_sub(saved.at.0) > longest;
            too_many || too_old
        })
        .map(|(_, saved)| saved.id)
        .collect()
}
