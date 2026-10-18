//! A conversation with an agent app moving between docket and the app's own command-line program
//! in a terminal, as the shell sees it: where a helper's conversation is now, what the person can
//! do about it, the conversations a terminal started that can be brought in, and what bringing
//! one in came to. Every sentence is plain words.

use prov::{AgentProgram, SessionId};
use serde::{Deserialize, Serialize};

/// Where a conversation with an agent app is running.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunsIn {
    /// In docket: the app runs here.
    #[default]
    Desktop,
    /// In the app's own program, in a terminal window.
    Terminal,
}

/// What the person can do about where a conversation is.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Handover {
    /// Nothing now: no conversation to move yet, or the app cannot pick it up again.
    #[default]
    Unavailable,
    /// "Open in Terminal" moves it to the app's own program.
    ToTerminal,
    /// "Bring Back" moves it into docket.
    BringBack,
}

/// A conversation that a terminal started with an agent app and the person can bring into docket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSession {
    /// The session docket keeps its restore points on; bringing it back names this.
    pub session: SessionId,
    /// The app.
    pub program: AgentProgram,
    /// What the person calls the app ("Claude Code"), when it is one we know the name of.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_name: Option<String>,
    /// The folder it works in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// `BringBack`, or `Unavailable` when docket cannot start the app here.
    #[serde(default)]
    pub handover: Handover,
}

/// What bringing a terminal's conversation into docket came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum BroughtBack {
    /// It is a conversation of docket's now, on this session.
    Session(SessionId),
    /// It stayed where it was, and why, in plain words.
    Refused(String),
}
