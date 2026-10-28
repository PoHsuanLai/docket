//! Who is the caller: the one type for the fact, shared by the wire, the router and the stored
//! turns. The role is a closed set the router branches on; it is configured per bus name in
//! `intentd.toml` and never sent in a body.

use porter_core::AppName;
use serde::{Deserialize, Serialize};

/// The role a connection plays, configured per bus name in `intentd.toml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerRole {
    /// sill's launcher: acts as the person.
    Launcher,
    /// A quire app recording a turn from its own prompt field.
    Field,
    /// A code editor driving the companion over ACP (`docket-acp`): opens sessions, records its
    /// person's turns and cancels. It answers no `Confirm1` sheet and halts nothing.
    Editor,
    /// companiond.
    Companion,
    /// readerd: the only caller that may resolve a handle.
    Reader,
    /// cuad.
    Cua,
    /// actions-mcp.
    Mcp,
    /// Any process running `quire-do`: connection-derived, like every role.
    Cli,
    /// sill as the only `Confirm1` server the router trusts.
    Confirm,
    /// The compositor fork: may halt.
    Compositor,
    /// sill's control centre: may halt and resume.
    Control,
    /// Any other app: its own actions, plus search, preview and suggest.
    App,
    /// The host of an external coding agent (`docket-agent`): opens the session of the agent it
    /// launched, records the person's turns, and makes the agent's calls (its file reads and
    /// writes, its commands, its permission requests) for it. It answers no `Confirm1` sheet
    /// unless the session says so, and halts nothing.
    AcpAgent,
}

impl CallerRole {
    /// Every role.
    pub const ALL: [CallerRole; 13] = [
        CallerRole::Launcher,
        CallerRole::Field,
        CallerRole::Editor,
        CallerRole::Companion,
        CallerRole::Reader,
        CallerRole::Cua,
        CallerRole::Mcp,
        CallerRole::Cli,
        CallerRole::Confirm,
        CallerRole::Compositor,
        CallerRole::Control,
        CallerRole::App,
        CallerRole::AcpAgent,
    ];
}

/// Who made a call or recorded a turn: a role, and the app behind the connection for the roles
/// that act for an app (a prompt field, an editor, an agent host). Other types that name a
/// caller derive from this one: `GrantCaller` (docket-permissions-core) is the key a grant is
/// stored under and `Speaker` (companion-core) is the policy subject of the companion's bus; both
/// are different facts with more or other data, filled from the connection.
///
/// A caller records a turn as the launcher, a terminal, a field, an editor or an agent host; the
/// constructors build only those, so the stored form below always reads back as the same caller.
/// Its serialized form is the stored turn's `from`: `{"kind":"launcher"}`, `{"kind":"terminal"}`,
/// `{"kind":"field","v":"org.quire.Mail"}`, and `editor` and `agent` the same way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "CallerWire", into = "CallerWire")]
pub struct Caller {
    role: CallerRole,
    app: Option<AppName>,
}

impl Caller {
    /// The launcher (sill), acting as the person.
    pub fn launcher() -> Self {
        Self {
            role: CallerRole::Launcher,
            app: None,
        }
    }

    /// A terminal (`quire-do ask`): the person at a keyboard, or whatever is typing in it.
    pub fn terminal() -> Self {
        Self {
            role: CallerRole::Cli,
            app: None,
        }
    }

    /// A prompt field inside `app`.
    pub fn field(app: AppName) -> Self {
        Self {
            role: CallerRole::Field,
            app: Some(app),
        }
    }

    /// A code editor, as `app`.
    pub fn editor(app: AppName) -> Self {
        Self {
            role: CallerRole::Editor,
            app: Some(app),
        }
    }

    /// The host of an external coding agent, as `app`.
    pub fn agent(app: AppName) -> Self {
        Self {
            role: CallerRole::AcpAgent,
            app: Some(app),
        }
    }

    /// The caller a turn is recorded from when a connection in `role`, as `app`, records it.
    /// The roles that may record a turn are the field, editor, agent host and terminal; any
    /// other is the launcher (the router refuses the rest before it records anything).
    pub fn of(role: CallerRole, app: &AppName) -> Self {
        match role {
            CallerRole::Field => Self::field(app.clone()),
            CallerRole::Editor => Self::editor(app.clone()),
            CallerRole::AcpAgent => Self::agent(app.clone()),
            CallerRole::Cli => Self::terminal(),
            _ => Self::launcher(),
        }
    }

    /// The role.
    pub fn role(&self) -> CallerRole {
        self.role
    }

    /// The app behind the caller: set for a field, an editor and an agent host.
    pub fn app(&self) -> Option<&AppName> {
        self.app.as_ref()
    }
}

/// The stored form of a [`Caller`]: the five callers that record a turn, tagged by kind.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
enum CallerWire {
    Launcher,
    Terminal,
    Field(AppName),
    Editor(AppName),
    Agent(AppName),
}

impl From<CallerWire> for Caller {
    fn from(wire: CallerWire) -> Self {
        match wire {
            CallerWire::Launcher => Self::launcher(),
            CallerWire::Terminal => Self::terminal(),
            CallerWire::Field(app) => Self::field(app),
            CallerWire::Editor(app) => Self::editor(app),
            CallerWire::Agent(app) => Self::agent(app),
        }
    }
}

impl From<Caller> for CallerWire {
    fn from(caller: Caller) -> Self {
        match (caller.role, caller.app) {
            (CallerRole::Field, Some(app)) => Self::Field(app),
            (CallerRole::Editor, Some(app)) => Self::Editor(app),
            (CallerRole::AcpAgent, Some(app)) => Self::Agent(app),
            (CallerRole::Cli, _) => Self::Terminal,
            _ => Self::Launcher,
        }
    }
}
