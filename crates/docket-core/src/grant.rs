//! Standing consent for actions: which app, data class and Space a caller may use at all. A
//! grant is set by the person and lives across tasks; a task policy narrows it per task.
//! Both must allow a call.

use porter_core::consent::{Grant, Usage};
use porter_core::{AppName, DataClass};
use prov::{ActionName, ActorKind, SpaceScope};
use serde::{Deserialize, Serialize};

/// What a grant binds to. The caller is derived from the connection, never sent.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ActionGrantKey {
    /// Who may act.
    pub caller: GrantCaller,
    /// The app that owns the action.
    pub owner: AppName,
    /// The whole app or one action.
    pub target: GrantTarget,
    /// The data class.
    pub class: DataClass,
    /// Interactive or background.
    pub usage: Usage,
    /// Which Spaces.
    pub space: SpaceScope,
}

/// The caller kinds a grant names. `Mcp` and `App` grants are per party, so the key carries
/// who; the others are one party each.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum GrantCaller {
    /// The companion's agents.
    Companion,
    /// The computer-use runtime.
    Cua,
    /// This external MCP client.
    Mcp(prov::ClientName),
    /// A process running `quire-do`.
    Cli,
    /// This app on its own.
    App(AppName),
    /// An editor speaking ACP (`zed`): the client of the person's own session.
    Editor(prov::ClientName),
    /// An external agent program the companion drives over ACP (`claude-code`).
    AcpAgent(ProgramName),
}

impl GrantCaller {
    /// The actor kind this caller acts as.
    pub fn kind(&self) -> ActorKind {
        match self {
            GrantCaller::Companion => ActorKind::Companion,
            GrantCaller::Cua => ActorKind::Cua,
            GrantCaller::Mcp(_) => ActorKind::Mcp,
            GrantCaller::Cli => ActorKind::Cli,
            GrantCaller::App(_) => ActorKind::App,
            // prov has no ACP kind yet (FINDINGS "acp-grants"): both act as an external client.
            GrantCaller::Editor(_) | GrantCaller::AcpAgent(_) => ActorKind::Mcp,
        }
    }
}

/// The name of an external agent program (`claude-code`): 1 to 64 characters, no control
/// characters. It names a program; it grants nothing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ProgramName(String);

/// Why text is not a program name.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a program name is 1 to 64 characters with no control characters")]
pub struct ProgramNameError;

impl ProgramName {
    /// `text` as a program name.
    pub fn parse(text: &str) -> Result<Self, ProgramNameError> {
        if text.is_empty() || text.len() > 64 || text.chars().any(char::is_control) {
            Err(ProgramNameError)
        } else {
            Ok(Self(text.to_owned()))
        }
    }

    /// The name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for ProgramName {
    type Error = ProgramNameError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<ProgramName> for String {
    fn from(name: ProgramName) -> String {
        name.0
    }
}

/// What a grant covers of an app.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum GrantTarget {
    /// "The companion may use Mail".
    App,
    /// One verb.
    Action(ActionName),
}

/// An action grant: porter's grant over docket's key.
pub type ActionGrant = Grant<ActionGrantKey>;
