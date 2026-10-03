//! Who is on the other end of a bus connection. The identity is derived here from what the bus
//! says about the connection and never from anything the caller sends: the well-known names the
//! connection owns, and, for a process that owns none (a terminal running `quire-do`), the
//! executable behind its pid. The roles are `intentd.toml`'s.
//!
//! This is advisory on a desktop where every process runs as the person: a process can take a
//! name or be named `quire-do`. What it cannot do is be more than the role that name plays, and
//! the cli role asks for everything that is not a read.

use crate::config::IntentdConfig;
use docket_core::CallerId;
use docket_dbus::BusConnection;
use porter_core::{AppId, AppName, Isolation};
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::sync::Arc;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

/// Executables that own no bus name and are named by what they are. A terminal's `quire-do` is
/// the cli role's one member.
const KNOWN_EXECUTABLES: [(&str, &str); 1] = [("quire-do", "org.quire.Do")];

/// Why a connection has no identity.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PeerFault {
    /// The connection is gone.
    #[error("the connection is gone")]
    Gone,
    /// It belongs to another user.
    #[error("another user's connection")]
    WrongUser,
    /// It owns no name intentd knows and runs no executable it knows.
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
    /// The file name of its executable, when readable.
    pub executable: Option<String>,
}

/// The caller `facts` make, with the roles of every name it plays; none for a connection that
/// is nobody intentd knows. The first name that has a role is the application; a connection
/// that owns no name is named by its executable.
pub(crate) fn derive(config: &IntentdConfig, facts: &Facts) -> Option<CallerId> {
    let by_executable = facts
        .executable
        .as_deref()
        .and_then(|exe| KNOWN_EXECUTABLES.iter().find(|(known, _)| *known == exe))
        .and_then(|(_, name)| AppName::parse(name).ok());
    let names: Vec<AppName> = if facts.names.is_empty() {
        by_executable.into_iter().collect()
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

/// The file name of the executable behind `pid`, or none when it cannot be read (the process
/// is gone, or is not ours).
fn executable_of(pid: u32) -> Option<String> {
    name_of(&std::fs::read_link(format!("/proc/{pid}/exe")).ok()?)
}

/// The file name of an executable's path; a binary replaced by an upgrade while it runs is
/// shown with a " (deleted)" suffix, and is still the same program.
fn name_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    Some(name.trim_end_matches(" (deleted)").to_owned())
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
}

impl Peers {
    /// Derives identities on `connection` with the roles of `config`.
    pub fn new(connection: BusConnection, config: Arc<IntentdConfig>) -> Self {
        Self { connection, config }
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
        let executable = credentials.process_id().and_then(executable_of);
        Ok(Facts { names, executable })
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
mod tests {
    use super::*;
    use docket_core::CallerRole;

    fn config() -> IntentdConfig {
        IntentdConfig::shipped().expect("the shipped configuration")
    }

    fn names(texts: &[&str]) -> Vec<AppName> {
        texts
            .iter()
            .map(|t| AppName::parse(t).expect("name"))
            .collect()
    }

    fn who(names_owned: &[&str], executable: Option<&str>) -> Option<(String, Vec<CallerRole>)> {
        let facts = Facts {
            names: names(names_owned),
            executable: executable.map(str::to_owned),
        };
        derive(&config(), &facts).map(|c| (c.app.name.to_string(), c.roles.into_iter().collect()))
    }

    #[test]
    fn a_table_of_connections_and_who_they_are() {
        /// One connection: what it owns, what it runs, and who it is.
        struct Row {
            what: &'static str,
            owned: &'static [&'static str],
            executable: Option<&'static str>,
            expected: Option<(&'static str, &'static [CallerRole])>,
        }
        let row = |what, owned, executable, expected| Row {
            what,
            owned,
            executable,
            expected,
        };
        const SILL: &[CallerRole] = &[
            CallerRole::Launcher,
            CallerRole::Confirm,
            CallerRole::Control,
        ];
        let rows = [
            row(
                "a terminal running quire-do owns no name and is the cli role",
                &[],
                Some("quire-do"),
                Some(("org.quire.Do", &[CallerRole::Cli])),
            ),
            row(
                "an unknown executable with no name is nobody",
                &[],
                Some("python3"),
                None,
            ),
            row("no name and no executable is nobody", &[], None, None),
            row(
                "sill owns several names and plays the roles of the one that is listed",
                &["org.quire.Confirm1", "org.quire.Shell"],
                Some("sill"),
                Some(("org.quire.Shell", SILL)),
            ),
            row(
                "an app that owns only its own name is a plain app",
                &["org.quire.Mail"],
                Some("mailo"),
                Some(("org.quire.Mail", &[])),
            ),
            row(
                "the name wins over the executable: an app called quire-do is still its own name",
                &["org.quire.Mail"],
                Some("quire-do"),
                Some(("org.quire.Mail", &[])),
            ),
            row(
                "companiond",
                &["org.quire.Companion1"],
                None,
                Some(("org.quire.Companion1", &[CallerRole::Companion])),
            ),
            row(
                "a name that is listed beats one that is not, whatever the order",
                &["org.aaa.First", "org.quire.Reader1"],
                None,
                Some(("org.quire.Reader1", &[CallerRole::Reader])),
            ),
            row(
                "an unlisted name alone is a plain app under that name",
                &["org.example.Thing"],
                None,
                Some(("org.example.Thing", &[])),
            ),
        ];
        for r in rows {
            let expected = r.expected.map(|(n, roles)| (n.to_owned(), roles.to_vec()));
            assert_eq!(who(r.owned, r.executable), expected, "{}", r.what);
        }
    }

    #[test]
    fn the_identity_is_unsandboxed_until_something_better_is_known() {
        let facts = Facts {
            names: names(&["org.quire.Mail"]),
            executable: None,
        };
        let caller = derive(&config(), &facts).expect("caller");
        assert_eq!(caller.app.isolation, Isolation::Unsandboxed);
    }

    #[test]
    fn an_executable_replaced_by_an_upgrade_is_still_the_same_program() {
        for (path, name) in [
            ("/usr/bin/quire-do", Some("quire-do")),
            ("/usr/bin/quire-do (deleted)", Some("quire-do")),
            ("/", None),
        ] {
            assert_eq!(name_of(Path::new(path)).as_deref(), name, "{path}");
        }
        assert!(executable_of(std::process::id()).is_some(), "our own");
    }
}
