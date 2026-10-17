//! The helpers on `org.quire.Companion1.Helpers`: what the shell lists and reads, and the forms
//! it sends. Every sentence in it is plain words: a helper has a name, a job, a model and a
//! pause; nothing here names how the machinery works.

use docket_core::LeadText;
use docket_core::helper::{
    Board, Ceiling, HelperCard, HelperModel, HelperName, HelperPhase, HelperState, JobId,
    JobStatus, Need, PauseWhy, Role, reach_lines,
};
use porter_core::{AppName, Count};
use prov::{AgentRef, HelperId, SpaceId};
use serde::{Deserialize, Serialize};

/// What a helper needs from the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperNeeds {
    /// A model to be chosen for it.
    ChooseModel,
    /// An answer to a question, or a yes on a sheet.
    Answer,
}

/// One helper in the list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperLine {
    /// Its id.
    pub id: HelperId,
    /// What the person calls it.
    pub name: HelperName,
    /// Its Space.
    pub space: SpaceId,
    /// What it is doing, in one line.
    pub doing: String,
    /// Where it stands.
    pub state: HelperPhase,
    /// What it needs from the person, if anything.
    pub needs_you: Option<HelperNeeds>,
    /// While it works, the agent the person's words to it are addressed to: a message to it
    /// goes through `Message.Send`, and `Companion1.Told` keeps their words on the roster.
    pub agent: Option<AgentRef>,
}

/// A sentence the person is told, once. The shell remembers the number of the last it showed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToldWire {
    /// Its number: later sentences have larger numbers.
    pub n: u64,
    /// The helper it is about.
    pub helper: HelperId,
    /// The sentence.
    pub text: String,
}

/// The list of helpers and what they said lately.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelpersView {
    /// One line each.
    pub helpers: Vec<HelperLine>,
    /// Sentences told since the number the shell asked after.
    pub told: Vec<ToldWire>,
}

/// Where a job stands, in the shell's few words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobShown {
    /// Waiting for its helper.
    Waiting,
    /// Being worked on.
    Working,
    /// Done.
    Done,
    /// Waiting for a model to be chosen.
    NeedsModel,
}

/// One job of a helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JobLine {
    /// Its id.
    pub id: JobId,
    /// What it is to do.
    pub goal: String,
    /// Where it stands.
    pub state: JobShown,
    /// Why its last try did not finish, in plain words.
    pub note: Option<String>,
}

/// A helper's card, as the shell shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperCardView {
    /// Its line in the list.
    pub line: HelperLine,
    /// What it is for, in the person's words.
    pub role: String,
    /// Its model: the person chooses it in the picker.
    pub model: HelperModel,
    /// What it may use, one plain line each.
    pub reach: Vec<String>,
    /// The most steps one job takes.
    pub steps: Count,
    /// Its jobs, oldest first.
    pub jobs: Vec<JobLine>,
}

/// What the person fills in to make or change a helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperForm {
    /// What they call it.
    pub name: HelperName,
    /// What it is for.
    pub role: Role,
    /// Its own Space.
    pub space: SpaceId,
    /// Its model; none until they choose.
    pub model: HelperModel,
    /// The apps it may use.
    pub apps: Vec<AppName>,
    /// How far it may go in them.
    pub ceiling: Ceiling,
    /// The most steps one job takes.
    pub steps: Count,
}

/// What a helper call came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum HelperReply {
    /// Done.
    Done,
    /// A helper was made.
    Made(HelperId),
    /// A job was posted.
    Posted(JobId),
    /// It was not done, and why, in plain words.
    Refused(String),
}

fn paused_text(why: PauseWhy) -> &'static str {
    match why {
        PauseWhy::ByPerson => "Paused",
        PauseWhy::Denied3 => "Paused: three of its requests in a row were refused",
        PauseWhy::Failed3 => "Paused: three of its jobs in a row did not finish",
    }
}

/// A helper as one line of the list. `answer` says whether the job it runs waits for the person
/// (a question or a sheet): only the host knows, from the task's answer.
pub fn line_of(card: &HelperCard, board: &Board, answer: Option<HelperNeeds>) -> HelperLine {
    let (state, doing, needs_you, agent) = match &card.state {
        HelperState::Idle => {
            let again = board
                .jobs()
                .iter()
                .any(|j| j.owner == card.id && j.status == JobStatus::Open && j.failed.is_some());
            let doing = if again {
                "A job did not finish; ask it to try again"
            } else {
                "Nothing to do"
            };
            (HelperPhase::Idle, doing.to_owned(), None, None)
        }
        HelperState::Working(at) => {
            let doing = board
                .job(at.job)
                .map(|j| LeadText::of(j.goal.as_str()).as_str().to_owned())
                .unwrap_or_default();
            (
                HelperPhase::Working,
                doing,
                answer,
                Some(AgentRef::Worker {
                    task: at.task.clone(),
                }),
            )
        }
        HelperState::Waiting(Need::ChooseModel) => (
            HelperPhase::Waiting,
            "Needs a model before it can start".to_owned(),
            Some(HelperNeeds::ChooseModel),
            None,
        ),
        HelperState::Paused(why) => (
            HelperPhase::Paused,
            paused_text(*why).to_owned(),
            None,
            None,
        ),
    };
    HelperLine {
        id: card.id.clone(),
        name: card.name.clone(),
        space: card.space.clone(),
        doing,
        state,
        needs_you,
        agent,
    }
}

fn job_line(job: &docket_core::helper::Job) -> JobLine {
    let state = match &job.status {
        JobStatus::Open => JobShown::Waiting,
        JobStatus::Taken(_) => JobShown::Working,
        JobStatus::Done(_) => JobShown::Done,
        JobStatus::Blocked(Need::ChooseModel) => JobShown::NeedsModel,
    };
    JobLine {
        id: job.id,
        goal: job.goal.as_str().to_owned(),
        state,
        note: job
            .failed
            .map(|fault| format!("It stopped: {}", fault.plain())),
    }
}

/// A helper's card as the shell shows it.
pub fn card_of(card: &HelperCard, board: &Board, answer: Option<HelperNeeds>) -> HelperCardView {
    HelperCardView {
        line: line_of(card, board, answer),
        role: card.role.as_str().to_owned(),
        model: card.model.clone(),
        reach: reach_lines(&card.reach),
        steps: card.budget.steps,
        jobs: board
            .jobs()
            .iter()
            .filter(|j| j.owner == card.id)
            .map(job_line)
            .collect(),
    }
}
