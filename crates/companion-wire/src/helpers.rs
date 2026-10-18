//! The helpers on `org.quire.Companion1.Helpers`: what the shell lists and reads, and the forms
//! it sends. Every sentence in it is plain words: a helper has a name, a job, a model and a
//! pause; nothing here names how the machinery works.

use docket_core::LeadText;
use docket_core::helper::{
    Board, Ceiling, HelperCard, HelperKind, HelperModel, HelperName, HelperPhase, HelperState,
    JobId, JobStatus, Need, PauseWhy, Role, SignInWay, TerminalRequest, reach_lines,
};
use crate::handoff::{Handover, RunsIn};
use porter_core::{AppName, Count};
use prov::{AgentRef, HelperId, SessionId, SpaceId};
use serde::{Deserialize, Serialize};

/// What a helper needs from the person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperNeeds {
    /// A model to be chosen for it.
    ChooseModel,
    /// The person to sign in to the app it runs, from its card.
    SignIn,
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
    /// The session the person's words to it are recorded on, when the companion keeps one: an
    /// editor lists the helper only with it. Absent from older lists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conversation: Option<SessionId>,
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
    /// Waiting for the person to sign in to the app the helper runs.
    NeedsSignIn,
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
    /// What it answered, while the answer is kept (a finished job's words are held for the jobs
    /// handed them, and not across a restart).
    #[serde(default)]
    pub answer: Option<String>,
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
    /// How it works: its own planner, or an app (Claude Code, say) that does its jobs. A card
    /// stored by a companion that knew only the first reads as it.
    #[serde(default)]
    pub kind: HelperKind,
    /// For a helper that runs an app: whether the person is signed in to it, as far as the
    /// companion knows. It is only ever known from the app: a session opened.
    #[serde(default)]
    pub login: LoginShown,
    /// For a helper that runs an app that wants signing in: the ways to, by the app's own names.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sign_in: Vec<SignInChoice>,
    /// For a helper that runs an app: what the person calls that app ("Claude Code"), when it is
    /// one we know the name of. The shell falls back to the program's own name when it is none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    /// For a helper that runs an app: whether the conversation is running in docket or in the
    /// app's own program in a terminal.
    #[serde(default)]
    pub runs_in: RunsIn,
    /// What the person can do about where the conversation is.
    #[serde(default)]
    pub handover: Handover,
}

/// What the person calls a coding agent program we know ("claude-code" is "Claude Code").
pub fn app_name_of(kind: &HelperKind) -> Option<String> {
    let name = match kind.program()?.as_str() {
        "claude-code" => "Claude Code",
        "gemini-cli" => "Gemini CLI",
        "codex" => "Codex",
        "antigravity" | "agy" => "Antigravity",
        _ => return None,
    };
    Some(name.to_owned())
}

/// Whether the person is signed in to the app a helper runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginShown {
    /// Not asked yet, or not an app that has a sign-in.
    #[default]
    Unknown,
    /// The app opened a session: it is signed in.
    SignedIn,
    /// The app wants the person to sign in; a job waits.
    NeedsSignIn,
}

/// One way of signing in to the app a helper runs, as the app names it ("Sign in with your
/// Claude account"). The shell shows the name; choosing it sends the id back.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SignInChoice {
    /// The id to send back.
    pub id: String,
    /// The app's own name for it.
    pub name: String,
}

impl From<&SignInWay> for SignInChoice {
    fn from(way: &SignInWay) -> Self {
        Self {
            id: way.id.clone(),
            name: way.name.clone(),
        }
    }
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
    /// How it works: its own planner unless the person picks an app. A form from a shell that
    /// knew only the first reads as it.
    #[serde(default)]
    pub kind: HelperKind,
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
    /// The shell is to open a terminal window running the app the helper runs, with what the app
    /// asked for added, inside the helper's sandbox and with the app's login folder, so the
    /// sign-in lands where the app reads it. When the window closes the shell asks the companion
    /// to check again (`Helpers.Resume` on the helper).
    OpenTerminal(TerminalRequest),
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
        // A job it has taken whose session is still opening: it is working on it already.
        HelperState::Idle if board.running(&card.id).is_some() => {
            let doing = board
                .running(&card.id)
                .map(|j| LeadText::of(j.goal.as_str()).as_str().to_owned())
                .unwrap_or_default();
            (HelperPhase::Working, doing, answer, None)
        }
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
        HelperState::Waiting(Need::SignIn) => (
            HelperPhase::Waiting,
            "Needs you to sign in before it can start".to_owned(),
            Some(HelperNeeds::SignIn),
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
        conversation: None,
    }
}

/// What companiond kept of a finished job: its answer, or the sentence that said why it stopped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobWords {
    /// What it answered.
    pub answer: Option<String>,
    /// Why it stopped, as told ("Qwen3 4B cannot answer: it cannot run right now.").
    pub why: Option<String>,
}

fn job_line(job: &docket_core::helper::Job, words: JobWords) -> JobLine {
    let state = match &job.status {
        JobStatus::Open => JobShown::Waiting,
        JobStatus::Taken(_) => JobShown::Working,
        JobStatus::Done(_) => JobShown::Done,
        JobStatus::Blocked(Need::ChooseModel) => JobShown::NeedsModel,
        JobStatus::Blocked(Need::SignIn) => JobShown::NeedsSignIn,
    };
    JobLine {
        id: job.id,
        goal: job.goal.as_str().to_owned(),
        state,
        // The sentence that said why names the model; the fault alone is the fallback.
        note: job.failed.map(|fault| {
            words
                .why
                .unwrap_or_else(|| format!("It stopped: {}", fault.plain()))
        }),
        answer: words.answer,
    }
}

/// A helper's card as the shell shows it; `kept` gives what is kept of each job's words.
pub fn card_of(
    card: &HelperCard,
    board: &Board,
    answer: Option<HelperNeeds>,
    kept: impl Fn(JobId) -> JobWords,
) -> HelperCardView {
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
            .map(|j| job_line(j, kept(j.id)))
            .collect(),
        kind: card.kind.clone(),
        app_name: app_name_of(&card.kind),
        login: match card.state {
            HelperState::Waiting(Need::SignIn) => LoginShown::NeedsSignIn,
            _ => LoginShown::Unknown,
        },
        sign_in: Vec::new(),
        runs_in: RunsIn::Desktop,
        handover: Handover::Unavailable,
    }
}
