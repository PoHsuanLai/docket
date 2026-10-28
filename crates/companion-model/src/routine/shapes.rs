//! What goes into and out of the routine book: a routine to make, a run to post, how a job
//! ended, the stored record and the one notice.

use super::kept::{Missed, Routine, RoutineId, RunBudget};
use super::trigger::Trigger;
use crate::helper::{GoalText, HelperName, JobFault};
use almanac_core::EventRef;
use prov::HelperId;
use serde::{Deserialize, Serialize};

/// A routine someone wants made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NewRoutine {
    /// The helper that does it.
    pub helper: HelperId,
    /// What to do: the person's words.
    pub goal: GoalText,
    /// What starts it.
    pub trigger: Trigger,
    /// What a run may use.
    pub budget: RunBudget,
    /// What a missed time does.
    pub missed: Missed,
}

/// What the book asks to have posted: an ordinary job on the helper. The goal is the person's
/// words; the event, when there is one, is a handle and is no part of the goal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRequest {
    /// The routine.
    pub routine: RoutineId,
    /// The helper that takes the job.
    pub helper: HelperId,
    /// What the job is to do.
    pub goal: GoalText,
    /// The event that started it.
    pub event: Option<EventRef>,
    /// What the run may use: the job stops at its steps.
    pub budget: RunBudget,
}

/// How a job on the board ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum JobEnd {
    /// It finished.
    Done,
    /// It did not.
    Failed(JobFault),
}

/// A routine's record: kept as it now stands, or gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RoutineRecord {
    /// The routine.
    Kept(Box<Routine>),
    /// The person removed it.
    Removed {
        /// Which.
        id: RoutineId,
        /// Its helper, whose Space the record belongs to.
        helper: HelperId,
    },
}

impl RoutineRecord {
    /// The helper whose Space the record belongs to.
    pub fn helper(&self) -> &HelperId {
        match self {
            RoutineRecord::Kept(routine) => &routine.helper,
            RoutineRecord::Removed { helper, .. } => helper,
        }
    }
}

/// Something worth telling the person about a routine, once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RoutineNotice {
    /// Three runs in a row did not finish, so it stopped running.
    Paused {
        /// When the routine runs, in plain words.
        when: String,
    },
}

impl RoutineNotice {
    /// The sentence, for a routine of the helper `name`.
    pub fn plain(&self, name: &HelperName) -> String {
        match self {
            RoutineNotice::Paused { when } => format!(
                "A routine of {} ({when}) is paused: three runs in a row did not finish. Resume it when you want it to try again.",
                name.as_str()
            ),
        }
    }
}
