//! Who is on the other end of a bus connection. The identity is derived here from what the bus
//! says about the connection and never from anything the caller sends, and never from a name it
//! owns: porter's `ProcCallers` (through [`Identity`]) names the process behind the connection
//! (the bus gives its pid; `/proc/<pid>/cgroup` names its unit or app scope; the callers file,
//! `intentd-callers.toml`, gives the role). The roles of `intentd.toml` are those of the app the
//! process is, and a role that stands for more than an app is played only by a process whose
//! callers-file role backs it ([`backs`]): the sheet roles (`launcher`, `confirm`, `control`) by
//! the `sheet_host` of the system's table, `cua` by `cua`, the other daemons' roles by a role
//! above a plain app. A scope never has one (porter gives a scope row a plain app's role).
//!
//! A connection nothing identifies is nobody and is refused, with one exception that is not a
//! name: a terminal's child (`vte-spawn-*`, `tmux-spawn-*`, a login `session-*` scope) has no
//! unit or app of its own, and is the cli role, whose one member is `org.quire.Do`:
//! - an app scope (`app-[<launcher>-]<id>-<n>.scope`, a Flatpak app by its metadata) is that
//!   app, with the roles `intentd.toml` lists under its name (none, for most);
//! - anything else (a service unit the table does not name, an unreadable cgroup) is refused.
//!
//! This is advisory on a desktop where every process runs as the person: a process can start a
//! scope. What it cannot do is be more than the role its unit, as the systemd manager names it,
//! plays, and the cli role asks for everything that is not a read. One narrowing:
//! `org.quire.Acp` counts as an app only while `agent.acp.expose` is on (`AcpGate`), because the
//! process is usually not there.

use crate::acp_gate::AcpGate;
use crate::config::IntentdConfig;
use docket_core::{CallerId, CallerRole};
use docket_dbus::BusConnection;
use docket_dbus::callers::{Caller, Callers, Identity, Role};
use docket_procroot::{own_uid, terminal_scope};
use porter_core::{AppId, AppName, Isolation};
use porter_daemon::ProcRoot;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

/// The name the cli role is listed under: what a terminal's child is.
const CLI_NAME: &str = "org.quire.Do";

/// Why a connection has no identity.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerFault {
    /// The connection is gone.
    #[error("the connection is gone")]
    Gone,
    /// It belongs to another user.
    #[error("another user's connection")]
    WrongUser,
    /// No process, unit or app scope intentd knows is behind it.
    #[error("an unknown caller")]
    Unknown,
    /// The bus failed.
    #[error("bus: {0}")]
    Bus(String),
}

/// What identified a connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Found {
    /// The callers table (a unit it names, an app scope, a Flatpak app) named the process.
    Process(Caller),
    /// Nothing named it, and its cgroup is a terminal child's scope.
    Terminal,
}

/// Whether a process the callers file gives the role `held` may play the intentd role `role`.
/// The sheet roles need the system's `sheet_host`, `cua` and `compositor` need their own, and
/// the roles of the other daemons and of the editor hosts need a role above a plain app (`agent`,
/// or any fixed role). The roles of a plain app, a terminal's child and a field are open to any
/// identified caller: `intentd.toml` lists who plays them.
pub(crate) fn backs(role: CallerRole, held: Role) -> bool {
    match role {
        CallerRole::Launcher | CallerRole::Confirm | CallerRole::Control => held == Role::SheetHost,
        CallerRole::Cua => held == Role::Cua,
        CallerRole::Compositor => held == Role::Compositor,
        CallerRole::Companion
        | CallerRole::Reader
        | CallerRole::Mcp
        | CallerRole::Editor
        | CallerRole::AcpAgent => held != Role::App,
        CallerRole::Field | CallerRole::Cli | CallerRole::App => true,
    }
}

/// The caller `found` makes, with the roles of its app that its role in the table backs; none for
/// a process that is nobody intentd knows. An ACP app that the gate has shut is nobody.
pub(crate) fn derive(config: &IntentdConfig, acp: &AcpGate, found: &Found) -> Option<CallerId> {
    let (app, held) = match found {
        Found::Process(caller) => {
            if !acp.keeps(&caller.app.name) {
                return None;
            }
            (caller.app.clone(), caller.role)
        }
        Found::Terminal => {
            let app = AppId {
                name: AppName::parse(CLI_NAME).ok()?,
                isolation: Isolation::Unsandboxed,
            };
            (app, Role::App)
        }
    };
    let roles = config
        .roles_of(&app.name)
        .into_iter()
        .filter(|role| backs(*role, held))
        .collect();
    Some(CallerId { app, roles })
}

/// What identifies a connection whose process is `pid` (as the bus names it) and which the callers
/// table named as `named`: that, else the terminal scope its cgroup is, else nothing.
pub(crate) fn classify(root: &Path, pid: Option<u32>, named: Option<Caller>) -> Option<Found> {
    match (named, pid) {
        (Some(caller), _) => Some(Found::Process(caller)),
        (None, Some(pid)) if terminal_scope(root, pid).is_some() => Some(Found::Terminal),
        (None, _) => None,
    }
}

/// Derives identities of the connections on one bus.
#[derive(Debug, Clone)]
pub struct Peers {
    connection: BusConnection,
    config: Arc<IntentdConfig>,
    who: Arc<Identity>,
    proc_root: PathBuf,
    acp: AcpGate,
}

impl Peers {
    /// Derives identities on `connection` with the roles of `config`, naming processes through
    /// `who`; terminals' cgroups are read from the system's `/proc`.
    pub fn new(connection: BusConnection, config: Arc<IntentdConfig>, who: Arc<Identity>) -> Self {
        Self::with_proc_root(connection, config, who, &ProcRoot::System)
    }

    /// As [`Peers::new`], reading terminals' cgroups from the proc root the daemon chose (a
    /// fixture tree in a test build).
    pub fn with_proc_root(
        connection: BusConnection,
        config: Arc<IntentdConfig>,
        who: Arc<Identity>,
        proc_root: &ProcRoot,
    ) -> Self {
        Self::gated(connection, config, who, proc_root, AcpGate::shut())
    }

    /// As [`Peers::with_proc_root`], honouring the roles held through `org.quire.Acp` only while
    /// `acp` shows the setting on.
    pub fn gated(
        connection: BusConnection,
        config: Arc<IntentdConfig>,
        who: Arc<Identity>,
        proc_root: &ProcRoot,
        acp: AcpGate,
    ) -> Self {
        Self {
            connection,
            config,
            who,
            proc_root: proc_root.path().to_path_buf(),
            acp,
        }
    }

    /// The roles `intentd.toml` gives.
    pub fn config(&self) -> &IntentdConfig {
        &self.config
    }

    /// What identifies the connection `unique`: the callers table, else a terminal's scope.
    async fn found(&self, unique: &str) -> Result<Found, PeerFault> {
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
        let named = self.who.caller_of(unique).await;
        classify(&self.proc_root, credentials.process_id(), named).ok_or(PeerFault::Unknown)
    }

    /// The caller behind a connection's unique name.
    pub async fn caller(&self, unique: &str) -> Result<CallerId, PeerFault> {
        let found = self.found(unique).await?;
        derive(&self.config, &self.acp, &found).ok_or(PeerFault::Unknown)
    }

    /// The caller that owns a well-known name right now: the name only addresses the connection,
    /// and whoever it is, the process behind it is identified as any other.
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
