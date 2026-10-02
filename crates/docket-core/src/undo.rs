//! The router's undo journal rows. The app's own stack stays the truth for Cmd+Z; the journal
//! labels who did what and lets the person undo a run or a task.

use crate::ids::{ActionRef, LabelText, UndoId, UndoToken};
use prov::{Actor, RunId, SessionId, TaskId, UnixSeconds};
use serde::{Deserialize, Serialize};

/// One undoable thing somebody did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoEntry {
    /// Its row.
    pub id: UndoId,
    /// When.
    pub at: UnixSeconds,
    /// Who did it.
    pub actor: Actor,
    /// The action.
    pub action: ActionRef,
    /// "Archived 3 threads", the app's words.
    pub said: LabelText,
    /// The app's token.
    pub token: UndoToken,
    /// The computer-use run, if one.
    pub run: Option<RunId>,
    /// The session.
    pub session: Option<SessionId>,
    /// Where it stands.
    pub state: UndoState,
}

/// Where an undo entry stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum UndoState {
    /// Can be undone.
    Available,
    /// Being undone.
    Undoing,
    /// Undone, by this actor.
    Undone {
        /// Who.
        by: Actor,
    },
    /// Could not be undone.
    Failed(UndoFault),
    /// Too old (the setting `agent.undo.keep_h`) or dropped by the app's stack.
    Expired,
}

/// Why an undo failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoFault {
    /// The thing is gone.
    Gone,
    /// It changed since.
    Conflict,
    /// The app is not running.
    AppUnavailable,
}

/// What to undo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum UndoScope {
    /// One entry.
    Entry(UndoId),
    /// A run's available entries, newest first, stopping at the first failure.
    Run(RunId),
    /// A task's.
    Task(TaskId),
}
