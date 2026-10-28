//! A helper's conversation with its agent app moving between docket and the app's own
//! command-line program in a terminal: what the host of the agent apps answers when the person
//! opens the conversation in a terminal and when they bring it back.

use super::agent::TerminalRun;
use prov::SessionId;
use serde::{Deserialize, Serialize};

/// Why a conversation could not move.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandFault {
    /// The app has no way to pick a conversation up again (its entry has no `resume`, or the
    /// app does not offer it).
    NotResumable,
    /// The app has not said its own id for the conversation yet.
    NoConversation,
    /// A job is running.
    Busy,
    /// The helper is paused.
    Paused,
    /// The conversation is not where it was expected to be.
    NotThere,
    /// The app could not be started or stopped, or the router would not move the session.
    Failed,
}

impl HandFault {
    /// What the person is told, in plain words.
    pub fn plain(self) -> &'static str {
        match self {
            HandFault::NotResumable => "This app can't pick its conversation back up here.",
            HandFault::NoConversation => {
                "There is no conversation to take along yet. Give it a job first."
            }
            HandFault::Busy => "It is busy with a job. Wait for it to finish.",
            HandFault::Paused => "It is paused. Resume it first.",
            HandFault::NotThere => "That conversation is not there any more.",
            HandFault::Failed => "It could not be moved. Try again.",
        }
    }
}

/// The conversation, ready to open in a terminal: the command that picks it up, and the session
/// at the router that is a watch session from now on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalHand {
    /// What to run: the app's own command-line program, resolved the way a sign-in is.
    pub run: TerminalRun,
    /// The session at the router.
    pub session: SessionId,
}

/// What opening a conversation in a terminal came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ToTerminalOutcome {
    /// The app is closed and the session is a watch session: open the terminal.
    Ready(Box<TerminalHand>),
    /// It stayed where it was.
    Refused(HandFault),
}

/// What bringing a conversation back came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum BackOutcome {
    /// The app is running again on the same session, on its own conversation.
    Back,
    /// It stayed in the terminal.
    Refused(HandFault),
}

/// What bringing back a conversation a terminal started came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum BroughtOutcome {
    /// The app is running on the watch session, which is a conversation of docket's now.
    Brought(SessionId),
    /// It stayed in the terminal.
    Refused(HandFault),
}
