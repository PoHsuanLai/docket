//! A helper's job as a value: the job to run, the session it opened, and what the run came to.
//! The runner that opens the session and drives the loop is `docket-tasks`'; the board's rules
//! for the report are `conclude`'s.

use companion_model::helper::{CallVerdict, HelperCard, Job, JobFault, SignInWay};
use prov::{EntityId, SessionId, TaskId};

/// An earlier job's answer, handed to a job as data: the session the answer was given in, and its
/// words. The router labels the words with what that session had read and delivers them to the
/// receiving planner as a handle, never as text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobInput {
    /// The session of the job that answered.
    pub from: SessionId,
    /// What it said.
    pub text: String,
}

/// A job to run.
#[derive(Debug, Clone)]
pub struct JobSpec {
    /// The helper.
    pub card: HelperCard,
    /// The job, as the board holds it.
    pub job: Job,
    /// Earlier answers it is handed.
    pub inputs: Vec<JobInput>,
}

/// A job whose session is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenedJob {
    /// The task it runs as.
    pub task: TaskId,
    /// Its session at the router.
    pub session: SessionId,
}

/// How a job's run ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobEnding {
    /// The helper answered (or asked the person something, which is its last word on the job).
    Done {
        /// The answer's words.
        words: String,
    },
    /// The agent app the helper runs wants the person to sign in first. The job did not fail.
    NeedsSignIn(Vec<SignInWay>),
    /// It did not finish.
    Failed(JobFault),
    /// It was stopped before it finished.
    Stopped,
}

/// What a finished run leaves for the board.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobReport {
    /// How it ended.
    pub ending: JobEnding,
    /// How each of its calls ended, as far as the breaker goes, in order.
    pub calls: Vec<CallVerdict>,
    /// The things it was refused because another job owns them.
    pub owned: Vec<EntityId>,
    /// A plain sentence on why a failed job failed, when the model's refusal gave one (the model
    /// that could not serve is named in it).
    pub why: Option<String>,
}

impl JobReport {
    /// A job that did not run at all.
    pub fn failed(fault: JobFault) -> Self {
        Self {
            ending: JobEnding::Failed(fault),
            calls: Vec::new(),
            owned: Vec::new(),
            why: None,
        }
    }
}
