//! Tasks: each is its own session with its own taint, budget and task policy. The router keeps
//! the table, the ledger of each task until it ends (it becomes an episode), and the roster.

use docket_core::{Roster, TaskLedger, TaskPolicy};
use prov::{AgentRef, ReportStatus, SessionId, SpaceId, TaskId};
use std::collections::BTreeMap;

/// Where a task is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TaskState {
    /// Being set up.
    Starting,
    /// Working.
    Working,
    /// Waiting for the person.
    NeedsYou,
    /// Paused.
    Paused,
    /// Over, and how: a worker's or run's final report says the same.
    Ended(ReportStatus),
}

/// One task as the router knows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRecord {
    /// The task.
    pub task: TaskId,
    /// The session it runs as.
    pub session: SessionId,
    /// Its agent.
    pub agent: AgentRef,
    /// The task that spawned it.
    pub parent: Option<TaskId>,
    /// Its Space.
    pub space: SpaceId,
    /// Where it is.
    pub state: TaskState,
    /// What the router remembers of it for its episode.
    pub ledger: TaskLedger,
}

/// Every task, by id.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskTable {
    tasks: BTreeMap<TaskId, TaskRecord>,
}

impl TaskTable {
    /// An empty table.
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a task.
    pub fn insert(&mut self, record: TaskRecord) {
        self.tasks.insert(record.task.clone(), record);
    }

    /// One task.
    pub fn get(&self, task: &TaskId) -> Option<&TaskRecord> {
        self.tasks.get(task)
    }

    /// One task, to change.
    pub fn get_mut(&mut self, task: &TaskId) -> Option<&mut TaskRecord> {
        self.tasks.get_mut(task)
    }

    /// Every task.
    pub fn all(&self) -> impl Iterator<Item = &TaskRecord> {
        self.tasks.values()
    }
}

/// The policy a child task starts with: the requested one cut down to the parent's, never
/// wider. A child of a task with no policy has none (every non-read call is outside).
pub fn child_policy(parent: &TaskPolicy, requested: &TaskPolicy) -> TaskPolicy {
    let _ = (parent, requested);
    todo!(
        "child_policy: intersect actions, kinds, patterns and expiry; min of ceiling and count; the child's own task, from and rationale"
    )
}

/// The roster as an agent working in `active` sees it: a task in the same Space shows its
/// goal, last step and what the person told it; a task in another Space shows presence only.
pub fn roster_of(table: &TaskTable, active: &SpaceId) -> Roster {
    let _ = (table, active);
    todo!(
        "roster_of: one line per task, state from TaskState, goal as a handle unless it is the person's own words; then Roster::seen_from(active)"
    )
}
