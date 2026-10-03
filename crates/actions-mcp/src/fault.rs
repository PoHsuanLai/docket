//! Why a tool call produced no outcome. Coarse on purpose: a client is not an oracle for the
//! policy, so a refusal says which kind it was and never which rule, reviewer or app text.

use docket_core::{AppRefusal, CallRefusal, DenyCode};
use serde::{Deserialize, Serialize};

pub use docket_core::{ArgsFault, TargetFault, Why};

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
