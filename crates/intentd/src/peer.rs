//! Who is on the other end of a bus connection. The identity is derived here from what the bus
//! says about the connection and never from anything the caller sends: the well-known names the
//! connection owns, and, for a process that owns none (a terminal running `quire-do`), the
//! cgroup behind its pid, read the way porter's daemons read it (`porter_dbus::ProcCallers`:
//! only `/proc/<pid>/cgroup`, never `exe`). The roles are `intentd.toml`'s.
//!
//! A connection with no name is identified by its cgroup alone:
//! - an app scope (`app-[<launcher>-]<id>-<n>.scope`, `app-flatpak-<id>-<n>.scope`) is that app,
//!   with the roles `intentd.toml` lists under its name (none, for most);
//! - a terminal's child (`vte-spawn-*`, `tmux-spawn-*`, a login `session-*` scope) is the cli
//!   role, the one member of which is `org.quire.Do`;
//! - anything else (a service unit, an unreadable cgroup) is nobody and is refused.
//!
//! This is advisory on a desktop where every process runs as the person: a process can take a
//! name or start a scope. What it cannot do is be more than the role that name plays, and the
//! cli role asks for everything that is not a read.

use crate::config::IntentdConfig;
use crate::procroot::ProcRoot;
use docket_core::CallerId;
use docket_dbus::BusConnection;
use porter_core::{AppId, AppName, CgroupPath, Isolation};
use porter_dbus::{CallerTable, ProcCallers};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

/// The name the cli role is listed under: what a terminal's child is.
const CLI_NAME: &str = "org.quire.Do";

/// Scope name prefixes of a terminal's children: a VTE terminal's spawned shell, tmux's server
/// and a login session (tty or ssh).
const TERMINAL_SCOPES: [&str; 3] = ["vte-spawn-", "tmux-spawn-", "session-"];

/// Why a connection has no identity.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerFault {
    /// The connection is gone.
    #[error("the connection is gone")]
    Gone,
    /// It belongs to another user.
    #[error("another user's connection")]
    WrongUser,
    /// It owns no name intentd knows and its cgroup names nobody.
    #[error("an unknown caller")]
    Unknown,
    /// The bus failed.
    #[error("bus: {0}")]
    Bus(String),
}

/// What the bus told about one connection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Facts {
    /// The application-shaped well-known names it owns.
    pub names: Vec<AppName>,
    /// What its cgroup says it is.
    pub process: Process,
}

/// What a process is, from its cgroup.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum Process {
    /// Nothing names it: a service unit, an unlisted scope, no readable cgroup.
    #[default]
    Unknown,
    /// An app's scope, named by it.
    App(AppName),
    /// A terminal's child.
    Terminal,
}

/// The caller `facts` make, with the roles of every name it plays; none for a connection that
/// is nobody intentd knows. The first name that has a role is the application; a connection
/// that owns no name is named by its cgroup.
pub(crate) fn derive(config: &IntentdConfig, facts: &Facts) -> Option<CallerId> {
    let by_process = match &facts.process {
        Process::App(name) => Some(name.clone()),
        Process::Terminal => AppName::parse(CLI_NAME).ok(),
        Process::Unknown => None,
    };
    let names: Vec<AppName> = if facts.names.is_empty() {
        by_process.into_iter().collect()
    } else {
        facts.names.clone()
    };
    let roles = names.iter().flat_map(|n| config.roles_of(n)).collect();
    let app = names
        .iter()
        .find(|n| !config.roles_of(n).is_empty())
        .or_else(|| names.first())?
        .clone();
    Some(CallerId {
        app: AppId {
            name: app,
            isolation: Isolation::Unsandboxed,
        },
        roles,
    })
}

/// What process `pid` of the `/proc` tree at `root` is, from its cgroup.
fn process_of(root: &Path, pid: u32) -> Process {
    let table = CallerTable::default();
    if let Some(found) = ProcCallers::caller_of_pid(root, pid, &table) {
        return Process::App(found.app.name);
    }
    let text =
        std::fs::read_to_string(root.join(pid.to_string()).join("cgroup")).unwrap_or_default();
    match CgroupPath::from_proc_cgroup(&text) {
        Ok(cgroup) if is_terminal_scope(cgroup.as_str()) => Process::Terminal,
        _ => Process::Unknown,
    }
}

/// Whether the cgroup's leaf is a terminal child's scope.
fn is_terminal_scope(cgroup: &str) -> bool {
    let leaf = cgroup.rsplit('/').next().unwrap_or_default();
    leaf.ends_with(".scope") && TERMINAL_SCOPES.iter().any(|p| leaf.starts_with(p))
}

/// The user this process runs as.
fn own_uid() -> Option<u32> {
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

/// Derives identities of the connections on one bus.
#[derive(Debug, Clone)]
pub struct Peers {
    connection: BusConnection,
    config: Arc<IntentdConfig>,
    proc_root: PathBuf,
}

impl Peers {
    /// Derives identities on `connection` with the roles of `config`, reading the system's
    /// `/proc`.
    pub fn new(connection: BusConnection, config: Arc<IntentdConfig>) -> Self {
        Self::with_proc_root(connection, config, &ProcRoot::System)
    }

    /// As [`Peers::new`], reading the proc root the daemon chose (a fixture tree in a test build).
    pub fn with_proc_root(
        connection: BusConnection,
        config: Arc<IntentdConfig>,
        proc_root: &ProcRoot,
    ) -> Self {
        Self {
            connection,
            config,
            proc_root: proc_root.path(),
        }
    }

    /// The roles `intentd.toml` gives.
    pub fn config(&self) -> &IntentdConfig {
        &self.config
    }

    async fn facts(&self, unique: &str) -> Result<Facts, PeerFault> {
        let bus = |e: zbus::Error| PeerFault::Bus(e.to_string());
        let dbus = DBusProxy::new(&self.connection).await.map_err(bus)?;
        let who = BusName::try_from(unique).map_err(|e| PeerFault::Bus(e.to_string()))?;
        let credentials = dbus
            .get_connection_credentials(who)
            .await
            .map_err(|_| PeerFault::Gone)?;
        if own_uid().is_some_and(|ours| credentials.unix_user_id() != Some(ours)) {
            return Err(PeerFault::WrongUser);
        }
        let mut names = Vec::new();
        for name in dbus.list_names().await.map_err(|e| bus(e.into()))? {
            let text = name.as_str();
            let Ok(app) = AppName::parse(text) else {
                continue;
            };
            if text.starts_with(':') {
                continue;
            }
            let owner = dbus
                .get_name_owner(BusName::try_from(text).map_err(|e| PeerFault::Bus(e.to_string()))?)
                .await;
            if owner.is_ok_and(|o| o.as_str() == unique) {
                names.push(app);
            }
        }
        names.sort();
        let process = credentials
            .process_id()
            .map_or(Process::Unknown, |pid| process_of(&self.proc_root, pid));
        Ok(Facts { names, process })
    }

    /// The caller behind a connection's unique name.
    pub async fn caller(&self, unique: &str) -> Result<CallerId, PeerFault> {
        let facts = self.facts(unique).await?;
        derive(&self.config, &facts).ok_or(PeerFault::Unknown)
    }

    /// The caller that owns a well-known name right now.
    pub async fn owner_of(&self, name: &str) -> Result<CallerId, PeerFault> {
        let dbus = DBusProxy::new(&self.connection)
            .await
            .map_err(|e| PeerFault::Bus(e.to_string()))?;
        let well_known = BusName::try_from(name).map_err(|e| PeerFault::Bus(e.to_string()))?;
        let owner = dbus
            .get_name_owner(well_known)
            .await
            .map_err(|_| PeerFault::Gone)?;
        self.caller(owner.as_str()).await
    }
}

#[cfg(test)]
mod tests;
