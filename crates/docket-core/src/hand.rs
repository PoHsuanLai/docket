//! A conversation with an agent app moving between docket and the app's own command-line
//! program in a terminal. The session at the router stays one session: it is a watch session
//! (restore points only) while the person works in the terminal, and an agent session again when
//! the conversation is brought back. These are the words for it that the router, the host of the
//! agent apps and the terminal share.

use crate::grant::ProgramName;
use crate::workspace::Workspace;
use prov::{AgentLabel, SessionId};
use serde::{Deserialize, Serialize};

/// An agent app and its own id for one conversation, which is what the app's command-line
/// program picks up again. Said by the host that ran the app or by the terminal that watches it;
/// never read from a model's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchedAgent {
    /// The configured program (`agents.toml`).
    pub program: ProgramName,
    /// The app's own id for the conversation, as the app prints it.
    pub session: String,
}

/// Where a conversation goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandTo {
    /// The app's own program in a terminal: the session becomes a watch session.
    Terminal,
    /// Back to docket: the session is an agent session again.
    Desktop,
}

/// What the log of a session says about where its conversation is, and with whom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandNote {
    /// Where the conversation went.
    pub to: HandTo,
    /// The app and its id for the conversation, when they are known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<WatchedAgent>,
}

/// One watch session whose agent a terminal named: what the shell can offer to bring back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchedLine {
    /// The watch session.
    pub session: SessionId,
    /// The app and its own id for the conversation.
    pub agent: WatchedAgent,
    /// What the person calls the agent there, as its opener said (display text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<AgentLabel>,
    /// The folder the agent works in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<Workspace>,
}
