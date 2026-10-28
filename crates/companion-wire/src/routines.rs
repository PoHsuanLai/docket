//! The routines on `org.quire.Companion1.Helpers`: what the shell lists and reads, and the form it
//! sends. Every sentence in it is plain words: a routine is something a helper does by itself
//! when a time comes, something happens, or another job is done.

use companion_model::helper::{HelperName, JobId, PauseWhy};
use companion_model::routine::{Missed, Routine, RoutineId, RoutineState, RunOutcome, RunRecord};
use prov::{HelperId, SpaceId, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::fmt;

/// Whether a routine runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RoutineShown {
    /// It runs when its time or event comes.
    On,
    /// The person paused it.
    Paused,
    /// Three runs in a row did not finish.
    Stopped,
}

/// How a run came out, in the shell's few words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunShown {
    /// Being worked on.
    Working,
    /// Done.
    Done,
    /// It did not finish.
    DidNotFinish,
    /// It did not run.
    Skipped,
}

/// One run of a routine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunLine {
    /// When.
    pub at: UnixSeconds,
    /// How it came out.
    pub state: RunShown,
    /// The job it became, while the helper's board holds it.
    pub job: Option<JobId>,
    /// Why it did not finish or did not run, in plain words.
    pub note: Option<String>,
}

/// One routine in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineLine {
    /// Its id.
    pub id: RoutineId,
    /// The helper that does it.
    pub helper: HelperId,
    /// What the person calls that helper.
    pub helper_name: HelperName,
    /// What it does, in the person's words.
    pub goal: String,
    /// When it runs, in plain words ("Every day at 8:00").
    pub when: String,
    /// Whether it runs.
    pub state: RoutineShown,
    /// The next time it is due, for a time rule.
    pub next: Option<UnixSeconds>,
    /// The last run, if any.
    pub last: Option<RunLine>,
}

/// The list of routines.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutinesView {
    /// One line each.
    pub routines: Vec<RoutineLine>,
}

/// A routine with its recent runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineCardView {
    /// Its line in the list.
    pub line: RoutineLine,
    /// What a time that is missed does.
    pub missed: Missed,
    /// The most steps one run takes.
    pub steps: u32,
    /// The last runs, newest first (at most 20).
    pub runs: Vec<RunLine>,
}

/// What starts a routine, as the person fills it in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum WhenForm {
    /// A time, in plain words: "daily 08:00", "weekdays 08:00", "every 2 hours".
    At(String),
    /// Something new in a Space.
    On(EventForm),
    /// Another job being done.
    AfterJob(JobId),
}

/// An event, as the shell names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventForm {
    /// The kind of event: a dotted name, or a prefix ending in `.*`.
    pub kind: String,
    /// The Space; none means the helper's own.
    pub space: Option<SpaceId>,
    /// What it is, in the person's words ("New mail in Inbox").
    pub words: String,
}

/// What the person fills in to make a routine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineForm {
    /// The helper that does it.
    pub helper: HelperId,
    /// What it is to do, in a few sentences.
    pub goal: String,
    /// What starts it.
    pub when: WhenForm,
    /// What a missed time does; skipping is the usual choice.
    #[serde(default)]
    pub missed: Missed,
    /// The most steps one run takes.
    pub steps: u32,
}

/// One routine the assistant (or a helper, for its own card) proposes, waiting for the person.
/// Nothing runs until they accept it on a sheet. Proposals are kept in memory only: a restart of
/// the assistant forgets them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoutineProposal {
    /// Its id, to accept or decline it.
    pub id: ProposalId,
    /// The helper that would do it.
    pub helper: HelperId,
    /// The routine as it would be made, for the sheet to show.
    pub form: RoutineForm,
    /// The proposer's one sentence on why. Words of a model, shown as data, never obeyed.
    pub why: String,
}

/// A proposal, written `p-3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProposalId(pub u64);

impl ProposalId {
    /// The id written `p-3`.
    pub fn parse(text: &str) -> Option<Self> {
        text.strip_prefix("p-")?.parse().ok().map(Self)
    }
}

impl fmt::Display for ProposalId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "p-{}", self.0)
    }
}

/// Text that is not a proposal id.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProposalIdFault;

impl fmt::Display for ProposalIdFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a proposal is written p-1, p-2 and so on")
    }
}

impl std::error::Error for ProposalIdFault {}

impl TryFrom<String> for ProposalId {
    type Error = ProposalIdFault;

    fn try_from(text: String) -> Result<Self, ProposalIdFault> {
        Self::parse(&text).ok_or(ProposalIdFault)
    }
}

impl From<ProposalId> for String {
    fn from(id: ProposalId) -> String {
        id.to_string()
    }
}

/// What a routine call came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RoutineReply {
    /// Done.
    Done,
    /// A routine was made.
    Made(RoutineId),
    /// It was not done, and why, in plain words.
    Refused(String),
}

/// Why a run did not run.
const SKIPPED: &str =
    "Its time came while the computer was asleep, or the helper could not take it";

fn run_line(run: &RunRecord) -> RunLine {
    let (state, note) = match run.outcome {
        RunOutcome::Started => (RunShown::Working, None),
        RunOutcome::Done => (RunShown::Done, None),
        RunOutcome::Failed(fault) => (
            RunShown::DidNotFinish,
            Some(format!("It stopped: {}", fault.plain())),
        ),
        RunOutcome::Skipped => (RunShown::Skipped, Some(SKIPPED.to_owned())),
    };
    RunLine {
        at: run.at,
        state,
        job: run.job,
        note,
    }
}

/// A routine as one line of the list; `helper_name` is what the person calls its helper.
pub fn routine_line(routine: &Routine, helper_name: HelperName) -> RoutineLine {
    RoutineLine {
        id: routine.id,
        helper: routine.helper.clone(),
        helper_name,
        goal: routine.goal.as_str().to_owned(),
        when: routine.trigger.plain(),
        state: match routine.state {
            RoutineState::Active => RoutineShown::On,
            RoutineState::Paused(PauseWhy::ByPerson) => RoutineShown::Paused,
            RoutineState::Paused(PauseWhy::Denied3 | PauseWhy::Failed3) => RoutineShown::Stopped,
        },
        next: routine.next_due,
        last: routine.runs.last().map(run_line),
    }
}

/// A routine's card: its line and its runs, newest first.
pub fn routine_card(routine: &Routine, helper_name: HelperName) -> RoutineCardView {
    RoutineCardView {
        line: routine_line(routine, helper_name),
        missed: routine.missed,
        steps: routine.budget.steps.0,
        runs: routine.runs.iter().rev().map(run_line).collect(),
    }
}
