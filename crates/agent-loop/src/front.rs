//! The front pointer: the task the launcher returns to. The companion is one identity over many
//! tasks; this is only which one the person is in.

use prov::TaskId;
use serde::{Deserialize, Serialize};

/// What moves the front pointer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum FrontEvent {
    /// The person asked something from the launcher: that task is now the front.
    Asked(TaskId),
    /// A task ended.
    Ended(TaskId),
    /// The person chose another task (a row, a card).
    Picked(TaskId),
}

/// One move. Ending a task that is not the front changes nothing; ending the front leaves no
/// front until the person asks again.
pub fn front_step(front: Option<TaskId>, event: &FrontEvent) -> Option<TaskId> {
    match event {
        FrontEvent::Asked(task) | FrontEvent::Picked(task) => Some(task.clone()),
        FrontEvent::Ended(task) if front.as_ref() == Some(task) => None,
        FrontEvent::Ended(_) => front,
    }
}
