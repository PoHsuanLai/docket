//! What happened to the workspace at a turn start, or at a restore.

use super::CheckpointId;
use crate::ids::TurnId;
use porter_core::UnixSeconds;
use serde::{Deserialize, Serialize};

/// Who keeps the history of an agent's file changes. Said by the host from `agents.toml`
/// (never by the agent); `Docket` is the default and over-saves rather than under-saves.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rewind {
    /// docket saves a restore point before each turn.
    #[default]
    Docket,
    /// The agent keeps its own history; docket saves nothing and says so.
    Agent,
}

/// Why a turn has no restore point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SkipReason {
    /// The agent keeps its own history.
    AgentKeepsOwn,
    /// The folder cannot keep restore points.
    NoHistory,
    /// The folder holds too many files.
    TooLarge,
    /// Saving went wrong or took too long.
    Failed,
}

/// What happened to the workspace at one turn start, or at a restore.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
#[non_exhaustive]
pub enum CheckpointEvent {
    /// A restore point was saved before the turn.
    Taken(CheckpointId),
    /// None was, for this reason.
    Skipped(SkipReason),
    /// The workspace was put back to `to`; `safety` is the point saved first so this can be undone.
    Restored {
        /// The point the workspace was put back to.
        to: CheckpointId,
        /// The point saved first.
        safety: CheckpointId,
    },
}

/// The log's note for one event (the payload of `SessionEntry::Checkpoint`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CheckpointNote {
    /// The turn it happened at.
    pub turn: TurnId,
    /// When.
    pub at: UnixSeconds,
    /// What.
    pub event: CheckpointEvent,
}
