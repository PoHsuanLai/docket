//! What a caller says to the companion, and where the launcher goes back to.

use docket_core::{TurnId, WindowKey};
use prov::{SessionId, TaskId};
use serde::{Deserialize, Serialize};

/// An ask: a session and a turn the UI already recorded through `Intents1.Session.Turn`, plus
/// the window to anchor a confirmation to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskWire {
    /// The session.
    pub session: SessionId,
    /// The turn.
    pub turn: TurnId,
    /// The window the person asked from.
    pub parent_window: WindowKey,
}

/// The one task the launcher returns to by default: the front thread's current task. companiond
/// owns the pointer; it moves when the person asks from the launcher and when the task ends.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrontTask {
    /// The task, if there is one.
    pub task: Option<TaskId>,
    /// Its session.
    pub session: Option<SessionId>,
}
