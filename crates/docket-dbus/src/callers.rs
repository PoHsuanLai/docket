//! Who is calling a daemon. One way, for every daemon of the agent tier: porter's
//! [`ProcCallers`] (the bus names the process behind a connection, `/proc/<pid>/cgroup` names the
//! unit or app scope, the daemon's callers file names the role) and a check of the [`Standing`] a
//! method asks for. A daemon never decides that a name it can see on the bus, the well-known name
//! someone owns, or a uid alone is a role; a caller nothing identifies is refused.
//!
//! The roles come from the daemon's own file, `<daemon>-callers.toml`: the system's under
//! `/etc/quire` and the person's under their configuration directory, laid over it
//! ([`read_table`]). Only the system's file can give a fixed role (`sheet_host`, `cua`, ...), and
//! a scope row is never more than a plain app, so the shell is `sill.service` and nothing else.
//!
//! A test (or a host that knows its own clients) names the callers of its private bus with
//! [`Identity::introduce_app`], because every connection of a test is one process; the rest are
//! read from a fake proc root. No daemon's main introduces anyone: there is no switch, in a cargo
//! feature or an environment variable, that does it for a shipped binary.

use crate::BusConnection;
pub use porter_dbus::callers::{
    Caller, CallerFileError, CallerRole as Role, CallerTable, Callers, ProcCallers, load_callers,
    table_from_toml,
};
use std::path::{Path, PathBuf};
use zbus::fdo;

/// Where the system's callers files are installed.
pub const SYSTEM_DIR: &str = "/etc/quire";

/// The person's configuration directory: `$XDG_CONFIG_HOME`, else `$HOME/.config`.
pub fn config_home(env: &impl Fn(&str) -> Option<String>) -> PathBuf {
    env("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            env("HOME")
                .map(PathBuf::from)
                .unwrap_or_default()
                .join(".config")
        })
}

/// The daemon's table: `/etc/quire/<file>` with `<config>/quire/<file>` laid over it. A file that
/// is missing is an empty table (nobody is identified); a file that does not parse is an error.
pub fn read_table(
    file: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<CallerTable, CallerFileError> {
    load_callers(
        &Path::new(SYSTEM_DIR).join(file),
        &config_home(env).join("quire").join(file),
    )
}

/// As [`read_table`], with the system's file at `system` instead of under `/etc/quire`: the
/// acceptance harness is its own system, and a fixed role comes from no other file.
pub fn read_table_at(
    system: &Path,
    file: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<CallerTable, CallerFileError> {
    load_callers(system, &config_home(env).join("quire").join(file))
}

/// What a method asks of its caller: the app its row names and the role the system's table gives
/// it. Both: a row cannot make an app the router by naming it, and an app scope that is called
/// like the router is only a plain app.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Standing {
    /// The app of the caller's row.
    pub app: &'static str,
    /// The role of the caller's row.
    pub role: Role,
}

impl Standing {
    /// The standing of `role` in the app `app`.
    pub const fn new(app: &'static str, role: Role) -> Self {
        Self { app, role }
    }

    /// Whether `caller` holds this standing.
    pub fn held_by(&self, caller: &Caller) -> bool {
        caller.app.name.as_str() == self.app && caller.role == self.role
    }
}

/// intentd, the router: the `agent` role of the app `org.quire.Intents`.
pub const INTENTD: Standing = Standing::new("org.quire.Intents", Role::Agent);
/// companiond, the assistant: the `agent` role of the app `org.quire.Companion1`. It is the only
/// caller of the agent host's `HandOver`.
pub const COMPANIOND: Standing = Standing::new("org.quire.Companion1", Role::Agent);
/// sill, the shell and the sheet host: the `sheet_host` role of the app `org.quire.Shell`.
pub const SHELL: Standing = Standing::new("org.quire.Shell", Role::SheetHost);

/// The table of a process that answers intentd alone and reads no file: one row, intentd's unit.
/// An app serving `IntentProvider1` has no `/etc/quire` file of its own to keep, and the router is
/// the same on every machine.
pub const INTENTD_ONLY: &str =
    "[[caller]]\napp = \"org.quire.Intents\"\nunit = \"intentd.service\"\nrole = \"agent\"\n";

/// The table of [`INTENTD_ONLY`]: one row, intentd's unit.
pub fn intentd_only() -> CallerTable {
    table_from_toml(INTENTD_ONLY).unwrap_or_default()
}

/// Why a caller was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    /// The message names no sender.
    #[error("no sender")]
    NoSender,
    /// Nothing identifies the process behind the connection.
    #[error("an unknown caller")]
    Unknown,
    /// An identified caller that does not hold the standing the method asks for.
    #[error("not the caller this method answers")]
    NotAllowed,
}

impl From<Refused> for fdo::Error {
    fn from(refused: Refused) -> Self {
        fdo::Error::AccessDenied(refused.to_string())
    }
}

/// The caller behind `sender`, if it holds one of the `allowed` standings.
pub async fn require(
    callers: &impl Callers,
    sender: Option<&str>,
    allowed: &[Standing],
) -> Result<Caller, Refused> {
    let sender = sender.ok_or(Refused::NoSender)?;
    let caller = callers.caller_of(sender).await.ok_or(Refused::Unknown)?;
    if allowed.iter().any(|standing| standing.held_by(&caller)) {
        Ok(caller)
    } else {
        Err(Refused::NotAllowed)
    }
}

/// Says who a bus connection is: porter's [`ProcCallers`] on the daemon's connection and table.
#[derive(Debug)]
pub struct Identity {
    procs: Procs,
    introduced: std::sync::Mutex<std::collections::BTreeMap<String, Caller>>,
}

/// Where the callers a test did not introduce come from.
#[derive(Debug)]
enum Procs {
    /// porter's reading of `/proc` and the unit manager, or of a fixture tree.
    Read(ProcCallers),
    /// Nobody: a fixture tree was named to a build that cannot read one (no `fixture-callers`),
    /// so no caller is identified rather than the system's `/proc` being read in its place.
    #[cfg(not(feature = "fixture-callers"))]
    Nobody,
}

impl Identity {
    /// Reads the system's `/proc`, and asks the systemd manager for a service's main process.
    pub fn on_system(connection: BusConnection, table: CallerTable) -> Self {
        Self::over(Procs::Read(ProcCallers::new(connection, table)))
    }

    /// Reads the fixture tree `proc_root` instead of `/proc` and the unit manager
    /// (`<root>/<pid>/cgroup`, `<root>/<pid>/exe`, and a service's main process in
    /// `<root>/units/<unit>`): how a test or the acceptance harness presents its processes. Only
    /// with the feature `fixture-callers`.
    #[cfg(feature = "fixture-callers")]
    pub fn on_fixture(connection: BusConnection, table: CallerTable, proc_root: PathBuf) -> Self {
        Self::over(Procs::Read(ProcCallers::with_proc_root(
            connection, table, proc_root,
        )))
    }

    /// The fixture tree when there is one, else the system's `/proc`. A build without
    /// `fixture-callers` that is handed a fixture tree identifies nobody.
    pub fn on(connection: BusConnection, table: CallerTable, fixture: Option<&Path>) -> Self {
        match fixture {
            #[cfg(feature = "fixture-callers")]
            Some(root) => Self::on_fixture(connection, table, root.to_owned()),
            #[cfg(not(feature = "fixture-callers"))]
            Some(_) => Self::over(Procs::Nobody),
            None => Self::on_system(connection, table),
        }
    }

    fn over(procs: Procs) -> Self {
        Self {
            procs,
            introduced: std::sync::Mutex::default(),
        }
    }

    /// Says that the connection `connection` is the app `app` with `role`, ahead of what `/proc`
    /// says: a test's stand-in for the unit or scope its process would be in.
    pub fn introduce_app(&self, connection: &BusConnection, app: &str, role: Role) {
        use porter_core::{AppId, AppName, Isolation};
        let (Some(unique), Ok(name)) = (connection.unique_name(), AppName::parse(app)) else {
            return;
        };
        let app = AppId {
            name,
            isolation: Isolation::Unsandboxed,
        };
        self.introduce(unique.as_str(), Caller { app, role });
    }

    /// Says that the connection `unique` is `caller`, ahead of what `/proc` says.
    pub fn introduce(&self, unique: &str, caller: Caller) {
        self.introduced
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(unique.to_owned(), caller);
    }
}

impl Callers for Identity {
    async fn caller_of(&self, sender: &str) -> Option<Caller> {
        let told = self
            .introduced
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(sender)
            .cloned();
        match told {
            Some(caller) => Some(caller),
            None => match &self.procs {
                Procs::Read(procs) => procs.caller_of(sender).await,
                #[cfg(not(feature = "fixture-callers"))]
                Procs::Nobody => None,
            },
        }
    }
}
