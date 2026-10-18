//! The board's jobs: one piece of work for one helper, with the words that tell it what to do and
//! where those words came from.

use super::card::Need;
use almanac_core::EventRef;
use porter_core::Count;
use prov::{EntityId, HelperId, SessionId, TaskId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

/// One job on the board.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct JobId(pub u64);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "j-{}", self.0)
    }
}

/// Why a goal is not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum GoalFault {
    /// Nothing but whitespace.
    #[error("the goal is empty")]
    Empty,
    /// Longer than allowed.
    #[error("the goal is too long")]
    TooLong,
}

/// The most characters of a goal, and of bytes (see the role's note on why bytes are bounded).
const GOAL_MAX: (usize, usize) = (1_000, 1_900);

/// What a job is to do, in words: an instruction to the helper that takes it. It is created only
/// from the person's words or from a planner that read nothing untrusted (see [`GoalSource`]).
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct GoalText(String);

impl GoalText {
    /// The goal written as `text`, or why it is not one.
    pub fn parse(text: &str) -> Result<Self, GoalFault> {
        let text = text.trim();
        if text.is_empty() {
            return Err(GoalFault::Empty);
        }
        if text.chars().count() > GOAL_MAX.0 || text.len() > GOAL_MAX.1 {
            return Err(GoalFault::TooLong);
        }
        Ok(Self(text.to_owned()))
    }

    /// The goal.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// The person's words: Debug shows the length only.
impl fmt::Debug for GoalText {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "GoalText(<{} chars>)", self.0.chars().count())
    }
}

impl TryFrom<String> for GoalText {
    type Error = GoalFault;
    fn try_from(text: String) -> Result<Self, GoalFault> {
        Self::parse(&text)
    }
}

impl From<GoalText> for String {
    fn from(goal: GoalText) -> String {
        goal.0
    }
}

/// Where a job's goal came from. Both reach the helper that takes the job as an instruction;
/// nothing else ever does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalSource {
    /// The person wrote it.
    Person,
    /// A planner wrote it, and nothing untrusted had reached that planner.
    PlannerOnTrusted,
}

/// What a planner that gives a job had seen, as the router computed it (never as it said).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlannerSaw {
    /// Nothing untrusted reached it.
    Nothing,
    /// Untrusted content reached it or its reader.
    Untrusted,
}

/// Who is giving a job, as far as its goal's trust goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum GoalOrigin {
    /// The person, through the shell.
    Person,
    /// A planner, having seen this.
    Planner(PlannerSaw),
}

/// What a finished job leaves: where its answer is held. The answer's words are the helper's own
/// and labelled by what it read; the board keeps no copy of them.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ResultHandle {
    /// The task that ran it.
    pub task: TaskId,
    /// The session that task ran in, from which its answer is handed on.
    pub session: SessionId,
}

/// Why a job did not finish, in a form the person can be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobFault {
    /// The work stopped unexpectedly.
    Crashed,
    /// The model gave nothing usable.
    NoAnswer,
    /// A step was refused and the work could not go on.
    Refused,
    /// It used up its steps.
    OutOfSteps,
    /// The app it runs is in the list of coding agents but is not installed here.
    NotInstalled,
    /// The app it runs is not in the list of coding agents yet.
    NotSetUp,
    /// Coding agents are turned off in Settings.
    TurnedOff,
}

impl JobFault {
    /// The fault as a clause that follows the helper's name ("Research stopped: …").
    pub fn plain(self) -> &'static str {
        match self {
            JobFault::Crashed => "the work stopped unexpectedly",
            JobFault::NoAnswer => "the model gave no usable answer",
            JobFault::Refused => "a step was refused and it could not go on",
            JobFault::OutOfSteps => "it used up its steps",
            JobFault::NotInstalled => "the app it runs is not installed on this computer",
            JobFault::NotSetUp => "the app it runs is not in the list of coding agents in Settings",
            JobFault::TurnedOff => "coding agents are turned off in Settings",
        }
    }
}

/// Where a job stands.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum JobStatus {
    /// Waiting for its helper.
    Open,
    /// Its helper took it.
    Taken(HelperId),
    /// Done; the answer is here.
    Done(ResultHandle),
    /// Waiting for something only the person can give.
    Blocked(Need),
}

/// One job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Job {
    /// Its id.
    pub id: JobId,
    /// What it is to do.
    pub goal: GoalText,
    /// Where the goal came from.
    pub source: GoalSource,
    /// The helper it is for.
    pub owner: HelperId,
    /// Where it stands.
    pub status: JobStatus,
    /// The things it writes: no other live job may write them.
    pub owns: BTreeSet<EntityId>,
    /// The task of the planner that gave it, whose policy it is cut from; none for the person's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub giver: Option<TaskId>,
    /// Finished jobs whose answers it is handed, as data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub with: Vec<JobId>,
    /// Why its last try did not finish; it is open again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub failed: Option<JobFault>,
    /// The almanac event that started it, when a routine's run: a handle, never part of the goal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub about: Option<EventRef>,
    /// The most steps this job takes, when a routine's run sets a budget of its own: the job
    /// stops at it, or at the helper's own, whichever is fewer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub steps: Option<Count>,
}

impl Job {
    /// Whether the job still holds the things it owns: anything but done.
    pub fn is_live(&self) -> bool {
        !matches!(self.status, JobStatus::Done(_))
    }
}
