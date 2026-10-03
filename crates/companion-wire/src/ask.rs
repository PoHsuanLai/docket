//! What a caller says to the companion, and where the launcher goes back to.

use docket_core::{ContextKeep, UserTurn, WindowKey};
use porter_core::AppName;
use prov::{SessionId, TaskId};
use serde::{Deserialize, Serialize};

/// An ask: a session and the turn the UI already recorded through `Intents1.Session.Turn`
/// (with the id the router gave it), which of the context the person kept, and where they asked
/// from: the window to anchor a confirmation to and the app whose context the companion reads.
/// companiond answers only the shell, which is who may speak for the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskWire {
    /// The session.
    pub session: SessionId,
    /// The turn, as recorded.
    pub turn: UserTurn,
    /// The context chips they kept.
    pub keep: ContextKeep,
    /// The window the person asked from.
    pub parent_window: WindowKey,
    /// The app they summoned the companion from; `None` for the launcher alone.
    pub app: Option<AppName>,
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
