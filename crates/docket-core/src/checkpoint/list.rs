//! A session's list of restore points.

use super::{CheckpointId, SkipReason, TurnState};
use crate::ids::TurnId;
use crate::units::Days;
use porter_core::{Count, UnixSeconds};
use prov::SessionId;
use serde::{Deserialize, Serialize};

/// Whether the store still holds a point (pruning removes it, the log keeps the note).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SavedState {
    /// The point can still be restored.
    Available,
    /// The point was cleared.
    Gone,
}

/// One row of the list, oldest first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub enum CheckpointRow {
    /// A point saved before a turn.
    Saved {
        /// Its number.
        id: CheckpointId,
        /// The turn it precedes.
        turn: TurnId,
        /// When it was saved.
        at: UnixSeconds,
        /// Whether it can still be restored.
        state: SavedState,
    },
    /// A turn with no point.
    NotSaved {
        /// The turn.
        turn: TurnId,
        /// When.
        at: UnixSeconds,
        /// Why.
        why: SkipReason,
    },
    /// The workspace was put back to a point.
    RestoredTo {
        /// The point.
        id: CheckpointId,
        /// When.
        at: UnixSeconds,
    },
}

/// What is kept: both limits apply (a point must be among the newest `last` AND younger than
/// `days`). The default is 20 and 14 (`agent.checkpoints.keep` and `.days`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Retention {
    /// How many of the newest points are kept.
    pub last: Count,
    /// How many days a point is kept.
    pub days: Days,
}

/// A session's list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointList {
    /// The session.
    pub session: SessionId,
    /// Its rows, oldest first.
    pub rows: Vec<CheckpointRow>,
    /// What is kept.
    pub keeps: Retention,
    /// Whether the session's agent is working now: edges grey out Restore while it is.
    #[serde(default)]
    pub turn: TurnState,
}
