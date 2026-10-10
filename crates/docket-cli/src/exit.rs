//! The exit codes of `quire-do`, stable and documented in `--help`, and the one error type every
//! stage of a run ends in. A code is derived from what the router said, never from prose.

use docket_client::{ClientError, TransportError};
use docket_core::{AppRefusal, CallRefusal, ConfirmEnd, UndoFault, WireRefusal};
use serde_json::{Value, json};

/// How a run ended, as the shell sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Exit {
    /// 0: done.
    Done,
    /// 2: usage: an unknown app, action or parameter, or a bad value.
    Usage,
    /// 3: refused by policy.
    Refused,
    /// 4: the person declined, or the confirmation timed out.
    Declined,
    /// 5: the app failed the call.
    AppFailed,
    /// 6: unavailable: intentd is down, or the app is not installed or does not start.
    Unavailable,
    /// 7: halted or paused (the kill switch, the breaker).
    Halted,
    /// 8: the companion waits for the person (a confirmation is answered in the shell).
    NeedsYou,
}

impl Exit {
    /// Every code, in order.
    pub const ALL: [Exit; 8] = [
        Exit::Done,
        Exit::Usage,
        Exit::Refused,
        Exit::Declined,
        Exit::AppFailed,
        Exit::Unavailable,
        Exit::Halted,
        Exit::NeedsYou,
    ];

    /// The process exit status.
    pub fn code(self) -> u8 {
        match self {
            Exit::Done => 0,
            Exit::Usage => 2,
            Exit::Refused => 3,
            Exit::Declined => 4,
            Exit::AppFailed => 5,
            Exit::Unavailable => 6,
            Exit::Halted => 7,
            Exit::NeedsYou => 8,
        }
    }

    /// A short word for the code, for the JSON error object.
    pub fn word(self) -> &'static str {
        match self {
            Exit::Done => "done",
            Exit::Usage => "usage",
            Exit::Refused => "refused",
            Exit::Declined => "declined",
            Exit::AppFailed => "app_failed",
            Exit::Unavailable => "unavailable",
            Exit::Halted => "halted",
            Exit::NeedsYou => "needs_you",
        }
    }
}

/// A run that did not end in `Exit::Done`: the code, a sentence for the terminal, and the
/// router's own words (a serialised refusal) for a program reading `--json`.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{what}")]
pub struct Failure {
    /// The exit code.
    pub exit: Exit,
    /// What happened, for a person.
    pub what: String,
    /// What the router said, as JSON, when it said anything.
    pub detail: Value,
}

impl Failure {
    /// A failure with a sentence and no detail.
    pub fn new(exit: Exit, what: impl Into<String>) -> Self {
        Self {
            exit,
            what: what.into(),
            detail: Value::Null,
        }
    }

    /// A usage error: the command line is wrong.
    pub fn usage(what: impl Into<String>) -> Self {
        Failure::new(Exit::Usage, what)
    }

    /// The error object of `--json`.
    pub fn json(&self) -> Value {
        json!({
            "vocab": docket_core::IntentsVocab::CURRENT,
            "error": {
                "exit": self.exit.code(),
                "kind": self.exit.word(),
                "what": self.what,
                "detail": self.detail,
            },
        })
    }
}

fn with_detail<T: serde::Serialize>(mut failure: Failure, detail: &T) -> Failure {
    failure.detail = serde_json::to_value(detail).unwrap_or(Value::Null);
    failure
}

/// How a call ended, when it did not end well: which code, and in which words.
pub fn of_refusal(refusal: &CallRefusal) -> Failure {
    let (exit, what) = match refusal {
        CallRefusal::App(AppRefusal::NeedsParam { param, .. }) => {
            (Exit::Usage, format!("the app needs {param}"))
        }
        CallRefusal::App(AppRefusal::NotFound(_)) => {
            (Exit::AppFailed, "the app cannot find that thing".to_owned())
        }
        CallRefusal::App(AppRefusal::Stale(_)) => (
            Exit::AppFailed,
            "that thing changed since it was named".to_owned(),
        ),
        CallRefusal::App(AppRefusal::Busy) => (Exit::AppFailed, "the app is busy".to_owned()),
        CallRefusal::App(AppRefusal::Unsupported) => {
            (Exit::AppFailed, "the app cannot do that".to_owned())
        }
        CallRefusal::App(AppRefusal::ClassificationChanged) => (
            Exit::AppFailed,
            "the app's state changed while the call was being checked".to_owned(),
        ),
        CallRefusal::App(AppRefusal::Failed(_)) => {
            (Exit::AppFailed, "the app failed the call".to_owned())
        }
        CallRefusal::Denied(_) => (Exit::Refused, "refused by policy".to_owned()),
        CallRefusal::OverBudget(_) => (Exit::Refused, "refused: over budget".to_owned()),
        CallRefusal::Unconfirmed(ConfirmEnd::Expired) => (
            Exit::Declined,
            "nobody answered the confirmation in time".to_owned(),
        ),
        CallRefusal::Unconfirmed(_) => (Exit::Declined, "the person declined".to_owned()),
        CallRefusal::Halted(_) => (Exit::Halted, "halted: everything is stopped".to_owned()),
        CallRefusal::Paused(_) => (
            Exit::Halted,
            "paused: too many refusals in a row; the person has to resume it".to_owned(),
        ),
        CallRefusal::NoSuchAction(_) => (Exit::Usage, "no such action".to_owned()),
        CallRefusal::BadArgs { param, why } => (
            Exit::Usage,
            format!("the argument {param} was refused: {}", slug(why)),
        ),
        CallRefusal::AppUnavailable(app) => (Exit::Unavailable, format!("{app} is not available")),
        CallRefusal::Timeout => (Exit::AppFailed, "the app did not answer in time".to_owned()),
        CallRefusal::NotRecorded => (
            Exit::Unavailable,
            "the session's record could not be kept, so nothing was done".to_owned(),
        ),
    };
    with_detail(Failure::new(exit, what), refusal)
}

fn slug<T: serde::Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s.replace('_', " "),
        _ => String::new(),
    }
}

/// A request that did not give the reply its caller wanted.
pub fn of_client(error: &ClientError) -> Failure {
    match error {
        ClientError::Transport(TransportError::Closed) => {
            Failure::new(Exit::Unavailable, "intentd is not running")
        }
        ClientError::Transport(TransportError::Bus(why)) => {
            Failure::new(Exit::Unavailable, format!("the bus failed: {why}"))
        }
        ClientError::Transport(TransportError::Malformed(_)) | ClientError::Unexpected => {
            Failure::new(Exit::Unavailable, "intentd answered something unexpected")
        }
        ClientError::Refused(WireRefusal::Call(refusal)) => of_refusal(refusal),
        ClientError::Refused(WireRefusal::NotAllowed) => {
            Failure::new(Exit::Refused, "refused: a terminal may not do that")
        }
        ClientError::Refused(WireRefusal::NoSuchSession) => {
            Failure::new(Exit::Unavailable, "the terminal's session is gone")
        }
        ClientError::Refused(WireRefusal::Malformed) => {
            Failure::new(Exit::Usage, "intentd could not read the request")
        }
        ClientError::Refused(WireRefusal::Send(_) | WireRefusal::Read(_)) => {
            Failure::new(Exit::AppFailed, "the request was refused")
        }
        // A failure this build does not know yet: report the link as not answering properly.
        _ => Failure::new(Exit::Unavailable, "intentd answered something unexpected"),
    }
}

/// How an undo that reached the app ended badly.
pub fn of_undo(fault: UndoFault) -> Failure {
    match fault {
        UndoFault::Gone => Failure::usage("there is nothing to undo for that"),
        UndoFault::Conflict => Failure::new(
            Exit::AppFailed,
            "the thing changed since, so the app cannot take it back",
        ),
        UndoFault::AppUnavailable => Failure::new(Exit::Unavailable, "the app is not available"),
    }
}
