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
    /// This app on its own.
    App(AppName),
}

impl GrantCaller {
    /// The actor kind this caller acts as.
    pub fn kind(&self) -> ActorKind {
        match self {
            GrantCaller::Companion => ActorKind::Companion,
            GrantCaller::Cua => ActorKind::Cua,
            GrantCaller::Mcp(_) => ActorKind::Mcp,
            GrantCaller::App(_) => ActorKind::App,
        }
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
