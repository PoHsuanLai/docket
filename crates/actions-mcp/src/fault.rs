//! Why a tool call produced no outcome. Coarse on purpose: a client is not an oracle for the
//! policy, so a refusal says which kind it was and never which rule, reviewer or app text.

use docket_core::{AppRefusal, CallRefusal, DenyCode, ParamName};
use serde::{Deserialize, Serialize};

/// What is wrong with a call's target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum TargetFault {
    /// The action acts on something and none was named.
    #[error("the action needs a target")]
    Missing,
    /// The action acts on nothing and one was named.
    #[error("the action takes no target")]
    Unexpected,
    /// A live text field cannot be named by a client.
    #[error("the target is a live text field, which a client cannot name")]
    NotNameable,
    /// The target is not of the shape (or the kind) the action declares.
    #[error("the target is malformed")]
    Malformed,
}

/// What is wrong with one argument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// The JSON is not of the declared type.
    #[error("wrong type")]
    Type,
    /// Outside the declared range, length or options.
    #[error("out of range")]
    Range,
}

/// Why the arguments of a call were refused, before anything was sent to the router.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ArgsFault {
    /// The arguments are not a JSON object.
    #[error("arguments must be an object")]
    NotAnObject,
    /// A name the action does not declare.
    #[error("unknown argument {0:?}")]
    Unknown(String),
    /// A required argument is absent.
    #[error("missing argument {0}")]
    Missing(ParamName),
    /// An argument is wrong.
    #[error("argument {param}: {why}")]
    Wrong {
        /// Which.
        param: ParamName,
        /// How.
        why: Why,
    },
    /// The target is wrong.
    #[error("target: {0}")]
    Target(#[from] TargetFault),
}

/// The ways the router refused a call, without its reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum McpRefusal {
    /// Policy or a reviewer said no; only the coarse code.
    #[error("denied ({0:?})")]
    Denied(DenyCode),
    /// The person was asked and did not say yes (or did not answer).
    #[error("not confirmed")]
    NotConfirmed,
    /// The Space is halted.
    #[error("halted")]
    Halted,
    /// The breaker paused the session.
    #[error("paused")]
    Paused,
    /// A budget ran out.
    #[error("over budget")]
    OverBudget,
    /// The router has no such action.
    #[error("no such action")]
    NoSuchAction,
    /// The router found an argument wrong.
    #[error("bad arguments")]
    BadArguments,
    /// The app is not there.
    #[error("app unavailable")]
    AppUnavailable,
    /// The app did not answer in time.
    #[error("timed out")]
    TimedOut,
    /// The app said no; what it said may quote the person's content and is never passed on.
    #[error("the app refused")]
    AppRefused,
}

impl From<&CallRefusal> for McpRefusal {
    fn from(refusal: &CallRefusal) -> Self {
        match refusal {
            CallRefusal::Denied(code) => McpRefusal::Denied(*code),
            CallRefusal::Unconfirmed(_) => McpRefusal::NotConfirmed,
            CallRefusal::Halted(_) => McpRefusal::Halted,
            CallRefusal::Paused(_) => McpRefusal::Paused,
            CallRefusal::OverBudget(_) => McpRefusal::OverBudget,
            CallRefusal::NoSuchAction(_) => McpRefusal::NoSuchAction,
            CallRefusal::BadArgs { .. } | CallRefusal::App(AppRefusal::NeedsParam { .. }) => {
                McpRefusal::BadArguments
            }
            CallRefusal::AppUnavailable(_) => McpRefusal::AppUnavailable,
            CallRefusal::Timeout => McpRefusal::TimedOut,
            CallRefusal::App(_) => McpRefusal::AppRefused,
        }
    }
}

/// Why a tool call gave no outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum McpFault {
    /// The name would be longer than 64 characters.
    #[error("tool name longer than 64 characters")]
    TooLong,
    /// The edge is off: MCP is off until the person turns it on.
    #[error("MCP is off")]
    Off,
    /// No offered action has this tool name.
    #[error("unknown tool")]
    UnknownTool,
    /// The arguments were refused before the router saw them.
    #[error("{0}")]
    Args(#[from] ArgsFault),
    /// The router refused the call.
    #[error("refused: {0}")]
    Refused(#[from] McpRefusal),
    /// The router is not reachable, or refused the request itself.
    #[error("the router is not available")]
    Unavailable,
}
