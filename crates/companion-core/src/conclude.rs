//! How a job's end is told to the board: the one decision the hub makes when the task that ran a
//! job is over. A run that ended in a words answer keeps its session open for its answer to be
//! handed on; every other end closes it.

use crate::job::{JobEnding, JobReport, OpenedJob};
use docket_core::helper::{BoardEvent, CallVerdict, JobFault, JobId, ResultHandle, SignInWay};
use prov::{EntityId, SessionId};

/// Whether the hub closes the session an answer was given in when the answer is let go. A native
/// job's session is the hub's to close; an agent app's session is its process's, kept open across
/// the helper's jobs, and ends with that process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hold {
    /// The hub closes the session.
    Close,
    /// The app's process owns the session.
    Leave,
}

/// What a finished job's answer is, while the process lives: the session it was given in and its
/// words. It is never written down: a handle to it expires with the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Answer {
    /// The session the answer was given in.
    pub session: SessionId,
    /// Its words.
    pub words: String,
    /// Who closes the session.
    pub hold: Hold,
}

/// A job's run, as the task that ran it ends: the session it opened, if it got that far, and what
/// the run came to.
#[derive(Debug, Clone)]
pub struct Finished {
    /// The session the run opened.
    pub opened: Option<OpenedJob>,
    /// What the run came to.
    pub report: JobReport,
    /// Who closes the session an answer was given in.
    pub hold: Hold,
}

/// What a job's end means for the board and for the router.
#[derive(Debug, Clone)]
pub struct Conclusion {
    /// What happens to the board.
    pub event: BoardEvent,
    /// The answer to keep, when there is one.
    pub answer: Option<Answer>,
    /// The session to close now.
    pub close: Option<SessionId>,
    /// How each call of the run ended, for the breaker.
    pub calls: Vec<CallVerdict>,
    /// The things the run was refused because another job owns them.
    pub owned: Vec<EntityId>,
    /// A plain sentence on why a failed job failed.
    pub why: Option<String>,
    /// The ways to sign in, when the job ended because its agent app wants the person to.
    pub ways: Option<Vec<SignInWay>>,
}

impl Conclusion {
    /// An end that keeps nothing: `event`, and `close` to be closed.
    fn bare(event: BoardEvent, close: Option<SessionId>) -> Self {
        Self {
            event,
            answer: None,
            close,
            calls: Vec::new(),
            owned: Vec::new(),
            why: None,
            ways: None,
        }
    }

    fn failed(job: JobId, fault: JobFault, close: Option<SessionId>) -> Self {
        Self::bare(BoardEvent::Failed { job, fault }, close)
    }

    /// A job whose task panicked: it failed, and only it.
    pub fn crashed(job: JobId, close: Option<SessionId>) -> Self {
        Self::failed(job, JobFault::Crashed, close)
    }

    /// A job whose task was cut off: it was stopped.
    pub fn cut_off(job: JobId, close: Option<SessionId>) -> Self {
        Self::bare(BoardEvent::Stopped(job), close)
    }
}

/// How a job's end is told to the board. A run that ended in a words answer keeps its session open
/// for its answer to be handed on; every other end closes it.
pub fn conclude(job: JobId, done: Finished) -> Conclusion {
    let Finished {
        opened,
        report,
        hold,
    } = done;
    let JobReport {
        ending,
        calls,
        owned,
        why,
    } = report;
    let close = opened.as_ref().map(|o| o.session.clone());
    let base = match (ending, opened) {
        (JobEnding::Done { words }, Some(opened)) => Conclusion {
            event: BoardEvent::Finished {
                job,
                result: ResultHandle {
                    task: opened.task,
                    session: opened.session.clone(),
                },
            },
            answer: Some(Answer {
                session: opened.session,
                words,
                hold,
            }),
            close: None,
            calls: Vec::new(),
            owned: Vec::new(),
            why: None,
            ways: None,
        },
        // Nothing failed: the job waits for the person to sign in, and so does its helper.
        (JobEnding::NeedsSignIn(ways), _) => Conclusion {
            event: BoardEvent::NeedsSignIn(job),
            ways: Some(ways),
            ..Conclusion::crashed(job, close)
        },
        (JobEnding::Done { .. }, None) => Conclusion::crashed(job, None),
        (JobEnding::Failed(fault), _) => Conclusion::failed(job, fault, close),
        (JobEnding::Stopped, _) => Conclusion::cut_off(job, close),
    };
    Conclusion {
        calls,
        owned,
        why,
        ..base
    }
}
