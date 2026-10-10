//! Whether an agent's turn is running, and how one ended.

use serde::{Deserialize, Serialize};

/// How a turn ended. Growing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum TurnEnd {
    /// The agent answered.
    Answered,
    /// The turn failed.
    Failed,
    /// The turn was cancelled, or the host stopped waiting for it.
    Cancelled,
}

/// Whether the session's agent is working now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    /// No turn is running; a restore may go ahead.
    #[default]
    Idle,
    /// A turn is running; a restore is refused.
    Running,
}
