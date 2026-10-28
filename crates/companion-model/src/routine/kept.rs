//! A routine as the person made it and as it now stands, with its last runs.

use super::trigger::Trigger;
use crate::helper::{GoalText, JobFault, JobId, PauseWhy};
use almanac_core::EventRef;
use porter_core::Count;
use prov::{HelperId, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The most run records a routine keeps.
pub const MOST_RUNS: usize = 20;

/// One routine of a helper.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RoutineId(pub u64);

impl RoutineId {
    /// The id written `r-3`.
    pub fn parse(text: &str) -> Option<Self> {
        text.strip_prefix("r-")?.parse().ok().map(Self)
    }
}

impl fmt::Display for RoutineId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r-{}", self.0)
    }
}

/// What a run may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunBudget {
    /// The most steps one run takes.
    pub steps: Count,
}

/// What happens to a time that came while the computer was asleep or the assistant was not
/// running. A missed time is skipped unless the person chose otherwise (owner, 2026-10-17).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Missed {
    /// Skip it and wait for the next time.
    #[default]
    Skip,
    /// Run once when the computer wakes.
    RunOnceOnWake,
}

/// Whether a routine runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RoutineState {
    /// It runs when its trigger comes.
    Active,
    /// It does not.
    Paused(PauseWhy),
}

/// How a run came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RunOutcome {
    /// Its job is posted and not yet finished.
    Started,
    /// Its job finished.
    Done,
    /// Its job did not finish.
    Failed(JobFault),
    /// It did not run: its time was missed, or the helper could not take the job.
    Skipped,
}

/// One run of a routine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    /// When it started, or when its time was skipped.
    pub at: UnixSeconds,
    /// The job on the helper's board, once posted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub job: Option<JobId>,
    /// The event that started it: a handle, never its content.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<EventRef>,
    /// How it came out.
    pub outcome: RunOutcome,
}

/// A standing instruction to a helper, with what starts it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Routine {
    /// Its id.
    pub id: RoutineId,
    /// The helper that does it.
    pub helper: HelperId,
    /// What to do: the person's words, written when the routine was made.
    pub goal: GoalText,
    /// What starts it.
    pub trigger: Trigger,
    /// What a run may use.
    pub budget: RunBudget,
    /// What a missed time does.
    pub missed: Missed,
    /// Whether it runs.
    pub state: RoutineState,
    /// For a time trigger, the next time it is due.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_due: Option<UnixSeconds>,
    /// For a routine that waits for a job: the job whose end started it, so one end starts it
    /// once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<JobId>,
    /// Runs that did not finish, in a row, since the last that did (or since it was resumed).
    #[serde(default)]
    pub failed: u8,
    /// The last runs, oldest first, at most [`MOST_RUNS`].
    #[serde(default)]
    pub runs: Vec<RunRecord>,
}

impl Routine {
    pub(super) fn record(&mut self, run: RunRecord) {
        self.runs.push(run);
        if self.runs.len() > MOST_RUNS {
            self.runs.remove(0);
        }
    }

    /// How many runs are not finished.
    pub fn running(&self) -> usize {
        self.runs
            .iter()
            .filter(|run| run.outcome == RunOutcome::Started)
            .count()
    }
}
