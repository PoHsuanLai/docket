//! The helper's card: what the person confirmed when they made it, and where it stands now.

use super::job::JobId;
use docket_permissions_core::TaskPolicy;
use porter_ids::Count;
use porter_infer::ModelRef;
use prov::{AgentProgram, HelperId, SpaceId, TaskId};
use serde::{Deserialize, Serialize};

/// Why a name or a role is not one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum HelperTextFault {
    /// Nothing but whitespace.
    #[error("it is empty")]
    Empty,
    /// Longer than allowed.
    #[error("it is too long")]
    TooLong,
    /// A control character where only words belong.
    #[error("it has a character that is not text")]
    NotText,
}

/// The most characters of a helper's name, and of bytes.
const NAME_MAX: (usize, usize) = (40, 120);
/// The most characters of a role, and of bytes. A helper's planner reads the role and the job's
/// goal at every turn in a text that is 4,000 bytes at most, so each is bounded in bytes too.
const ROLE_MAX: (usize, usize) = (600, 1_400);

fn checked(text: &str, max: (usize, usize), newlines: bool) -> Result<String, HelperTextFault> {
    let text = text.trim();
    if text.is_empty() {
        return Err(HelperTextFault::Empty);
    }
    if text.chars().count() > max.0 || text.len() > max.1 {
        return Err(HelperTextFault::TooLong);
    }
    if text
        .chars()
        .any(|c| c.is_control() && !(newlines && c == '\n'))
    {
        return Err(HelperTextFault::NotText);
    }
    Ok(text.to_owned())
}

/// What the person calls a helper ("Inbox", "Research"): one line, up to 40 characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct HelperName(String);

impl HelperName {
    /// The name written as `text`, or why it is not one.
    pub fn parse(text: &str) -> Result<Self, HelperTextFault> {
        checked(text, NAME_MAX, false).map(Self)
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for HelperName {
    type Error = HelperTextFault;
    fn try_from(text: String) -> Result<Self, HelperTextFault> {
        Self::parse(&text)
    }
}

impl From<HelperName> for String {
    fn from(name: HelperName) -> String {
        name.0
    }
}

/// What a helper is for, in the person's own words: trusted text, up to 600 characters.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Role(String);

impl Role {
    /// The role written as `text`, or why it is not one.
    pub fn parse(text: &str) -> Result<Self, HelperTextFault> {
        checked(text, ROLE_MAX, true).map(Self)
    }

    /// The role.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Role {
    type Error = HelperTextFault;
    fn try_from(text: String) -> Result<Self, HelperTextFault> {
        Self::parse(&text)
    }
}

impl From<Role> for String {
    fn from(role: Role) -> String {
        role.0
    }
}

/// How a helper works. On-screen helpers come as a further kind when they are built, not before.
/// A card stored before a kind was added reads as it was written: `"native"` stays `Native`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperKind {
    /// Our planner, on a model the person chose, acting only through apps' actions.
    #[default]
    Native,
    /// An agent app (Claude Code, Codex, any coding agent in the agent list) that does the jobs
    /// with its own model and its own sign-in. Its jobs reach it as turns and its words come back
    /// as untrusted text; every effect it asks for is ruled like a native helper's.
    AgentApp(AgentProgram),
}

impl HelperKind {
    /// The agent app this helper runs, when it is one.
    pub fn program(&self) -> Option<&AgentProgram> {
        match self {
            HelperKind::Native => None,
            HelperKind::AgentApp(program) => Some(program),
        }
    }
}

/// The model a helper's jobs run on. There is no default: a helper with no model does not run.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum HelperModel {
    /// The person has not chosen. Its jobs wait and ask them to.
    Unset,
    /// This one, and no other: a refusal names it, nothing stands in for it.
    Named(ModelRef),
}

/// What one job of a helper may spend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HelperBudget {
    /// The most planner turns one job takes.
    pub steps: Count,
}

/// What a helper waits for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Need {
    /// A model: the person has to choose one for it.
    ChooseModel,
    /// The agent app it runs is not signed in: the person signs in to it from the helper's card.
    SignIn,
}

/// The job a helper is working on, and the task it runs as.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TaskRef {
    /// The board's job.
    pub job: JobId,
    /// The task it runs as.
    pub task: TaskId,
}

/// Why a helper stopped taking jobs, or a routine stopped running.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PauseWhy {
    /// The person paused it.
    ByPerson,
    /// Three of its calls in a row were refused (a helper only).
    Denied3,
    /// Three of its jobs in a row failed.
    Failed3,
}

/// Where a helper stands.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum HelperState {
    /// Free to take a job.
    Idle,
    /// Working on one.
    Working(TaskRef),
    /// Waiting for something only the person can give.
    Waiting(Need),
    /// Not taking jobs.
    Paused(PauseWhy),
}

/// Where a helper stands, in a few words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperPhase {
    /// Ready for a job.
    Idle,
    /// Working on one.
    Working,
    /// Waiting for something only the person can give.
    Waiting,
    /// Not taking jobs.
    Paused,
}

impl HelperState {
    /// Where the helper stands, without the detail.
    pub fn phase(&self) -> HelperPhase {
        match self {
            HelperState::Idle => HelperPhase::Idle,
            HelperState::Working(_) => HelperPhase::Working,
            HelperState::Waiting(_) => HelperPhase::Waiting,
            HelperState::Paused(_) => HelperPhase::Paused,
        }
    }

    /// Whether the helper may be given a job now. A helper waiting for a model is asked again
    /// each time: the person may have chosen one.
    pub fn takes_jobs(&self) -> bool {
        matches!(self, HelperState::Idle | HelperState::Waiting(_))
    }
}

/// What the person confirms when they make or edit a helper: everything on the card but its id
/// and where it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardDraft {
    /// What the person calls it.
    pub name: HelperName,
    /// What it is for.
    pub role: Role,
    /// How it works.
    pub kind: HelperKind,
    /// Its own Space.
    pub space: SpaceId,
    /// Its model.
    pub model: HelperModel,
    /// The ceiling of every task it runs: a job's policy is cut to this.
    pub reach: TaskPolicy,
    /// What one job may spend.
    pub budget: HelperBudget,
}

/// A helper.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperCard {
    /// Its id.
    pub id: HelperId,
    /// What the person calls it.
    pub name: HelperName,
    /// What it is for.
    pub role: Role,
    /// How it works.
    pub kind: HelperKind,
    /// Its own Space.
    pub space: SpaceId,
    /// Its model.
    pub model: HelperModel,
    /// The ceiling of every task it runs.
    pub reach: TaskPolicy,
    /// What one job may spend.
    pub budget: HelperBudget,
    /// Where it stands.
    pub state: HelperState,
}

impl HelperCard {
    /// The card of a helper just confirmed: idle.
    pub fn new(id: HelperId, draft: CardDraft) -> Self {
        Self {
            id,
            name: draft.name,
            role: draft.role,
            kind: draft.kind,
            space: draft.space,
            model: draft.model,
            reach: draft.reach,
            budget: draft.budget,
            state: HelperState::Idle,
        }
    }

    /// The job it is working on, if it is.
    pub fn working_on(&self) -> Option<&TaskRef> {
        match &self.state {
            HelperState::Working(at) => Some(at),
            _ => None,
        }
    }
}
