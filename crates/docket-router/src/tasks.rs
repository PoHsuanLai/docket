//! Tasks: each is its own session with its own taint, budget and task policy. The router keeps
//! the table, the ledger of each task until it ends (it becomes an episode), and the roster.

use docket_core::{
    LeadText, Reveal, Roster, RosterDetail, RosterFull, RosterLine, RosterState, StepLine,
    TaskLedger, TaskPolicy, intersection,
};
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
    /// What it is for: the person's own words are plain, a planner's goal a handle held by the
    /// session that spawned it.
    pub goal: Reveal<String>,
    /// How its latest call ended.
    pub last: Option<StepLine>,
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
    intersection(requested, parent)
}

impl TaskState {
    /// How the roster words this state.
    pub fn roster_state(self) -> RosterState {
        match self {
            TaskState::Starting => RosterState::Starting,
            TaskState::Working | TaskState::Ended(ReportStatus::Progress) => RosterState::Working,
            TaskState::NeedsYou => RosterState::NeedsYou,
            TaskState::Paused => RosterState::Paused,
            TaskState::Ended(ReportStatus::Done) => RosterState::Done,
            TaskState::Ended(ReportStatus::Failed) => RosterState::Failed,
            TaskState::Ended(ReportStatus::Cancelled) => RosterState::Cancelled,
        }
    }
}

/// The roster as an agent working in `active` sees it: a task in the same Space shows its
/// goal, last step and what the person told it; a task in another Space shows presence only.
pub fn roster_of(table: &TaskTable, active: &SpaceId) -> Roster {
    let entries = table
        .all()
        .map(|t| RosterLine {
            agent: t.agent.clone(),
            space: t.space.clone(),
            state: t.state.roster_state(),
            detail: RosterDetail::Full(Box::new(RosterFull {
                goal: t.goal.clone(),
                last: t.last.clone(),
                told: t.ledger.asked.last().map(|turn| LeadText::of(&turn.text)),
            })),
        })
        .collect();
    Roster { entries }.seen_from(active)
}
