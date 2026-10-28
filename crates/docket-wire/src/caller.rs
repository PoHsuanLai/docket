//! Who is calling the router. Roles come from
//! `intentd.toml` and the connection's identity; they are never sent in a body.

use porter_core::AppId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

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

/// Scope name prefixes of a terminal's children: a VTE terminal's spawned shell, tmux's server
/// and a login session (tty or ssh).
const TERMINAL_SCOPES: [&str; 3] = ["vte-spawn-", "tmux-spawn-", "session-"];

/// Whether the cgroup path (the unified line of `/proc/<pid>/cgroup`) ends in a terminal child's
/// scope. The one place the desktop says what a terminal is: intentd derives the cli role from it
/// and companiond lets the same processes open, ask and close.
pub fn is_terminal_scope(cgroup: &str) -> bool {
    let leaf = cgroup.rsplit('/').next().unwrap_or_default();
    leaf.ends_with(".scope") && TERMINAL_SCOPES.iter().any(|p| leaf.starts_with(p))
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

/// A caller as the transport derived it: an identity and the roles `intentd.toml` gives that
/// bus name. One name may play several (sill is the launcher, the confirm server and the
/// control centre); the role a call acts in is the first that may make it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallerId {
    /// The application, as porter derives it from the connection.
    pub app: AppId,
    /// The roles of the name; empty means a plain app.
    pub roles: BTreeSet<CallerRole>,
}
