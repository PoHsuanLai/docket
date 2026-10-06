//! Who is on the other end of a bus connection. The identity is derived from what the bus says
//! about the connection and never from anything the caller sends: the application-shaped
//! well-known names it owns and, for a process that owns none, the app scope its cgroup names
//! (`porter_dbus::ProcCallers`: only `/proc/<pid>/cgroup`, never `exe`). The roles are
//! `voiced.toml`'s. This is advisory on a desktop where every process runs as the person: what a
//! process cannot do is be more than the role its name plays, and only the shell begins.

use crate::config::{VoiceRole, VoicedConfig};
use porter_core::AppName;
use porter_dbus::{CallerTable, ProcCallers};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

/// What the bus told about one connection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Facts {
    /// The application-shaped well-known names it owns.
    pub names: Vec<AppName>,
    /// The app its cgroup names, if it names one.
    pub scope: Option<AppName>,
}

/// The role `facts` give: a connection that owns a name the config lists as the shell is the
/// shell; a connection that owns none is named by its cgroup. `App` is never derived: it is
/// whoever a route names.
pub(crate) fn derive(config: &VoicedConfig, facts: &Facts) -> Option<VoiceRole> {
    let by_scope: Vec<&AppName> = facts.scope.iter().collect();
    let names: Vec<&AppName> = if facts.names.is_empty() {
        by_scope
    } else {
        facts.names.iter().collect()
    };
    names
        .into_iter()
        .any(|name| config.role_of(name) == Some(VoiceRole::Shell))
        .then_some(VoiceRole::Shell)
}

fn own_uid() -> Option<u32> {
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
}

fn scope_of(root: &Path, pid: u32) -> Option<AppName> {
    ProcCallers::caller_of_pid(root, pid, &CallerTable::default()).map(|found| found.app.name)
}

/// Derives the roles of the connections on one bus.
#[derive(Debug, Clone)]
pub struct Peers {
    connection: zbus::Connection,
    config: Arc<VoicedConfig>,
    proc_root: PathBuf,
}

impl Peers {
    /// Roles on `connection` by `config`, reading `proc_root` (the system's `/proc` in the
    /// daemon, a fixture tree in a test).
    pub fn new(
        connection: zbus::Connection,
        config: Arc<VoicedConfig>,
        proc_root: PathBuf,
    ) -> Self {
        Self {
            connection,
            config,
            proc_root,
        }
    }

    async fn facts(&self, unique: &str) -> zbus::Result<Option<Facts>> {
        let dbus = DBusProxy::new(&self.connection).await?;
        let who = BusName::try_from(unique)?;
        let credentials = dbus.get_connection_credentials(who).await?;
        if own_uid().is_some_and(|ours| credentials.unix_user_id() != Some(ours)) {
            return Ok(None);
        }
        let mut names = Vec::new();
        for name in dbus.list_names().await? {
            let text = name.as_str();
            let Ok(app) = AppName::parse(text) else {
                continue;
            };
            if text.starts_with(':') {
                continue;
            }
            let owner = dbus.get_name_owner(BusName::try_from(text)?).await;
            if owner.is_ok_and(|o| o.as_str() == unique) {
                names.push(app);
            }
        }
        names.sort();
        let scope = credentials
            .process_id()
            .and_then(|pid| scope_of(&self.proc_root, pid));
        Ok(Some(Facts { names, scope }))
    }

    /// The role of the connection `unique`: none for a stranger, another user's, or one that
    /// vanished.
    pub async fn role(&self, unique: &str) -> Option<VoiceRole> {
        let facts = self.facts(unique).await.ok().flatten()?;
        derive(&self.config, &facts)
    }

    /// Whether `unique` owns the well-known name `name` right now.
    pub async fn owns(&self, unique: &str, name: &AppName) -> bool {
        let Ok(dbus) = DBusProxy::new(&self.connection).await else {
            return false;
        };
        let Ok(well_known) = BusName::try_from(name.as_str()) else {
            return false;
        };
        dbus.get_name_owner(well_known)
            .await
            .is_ok_and(|owner| owner.as_str() == unique)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> VoicedConfig {
        VoicedConfig::parse(
            "earcons = \"on\"\n[roles]\nshell = [\"org.quire.Shell\"]\napp = [\"org.quire.Mailo\"]\n",
        )
        .expect("config")
    }

    fn name(text: &str) -> AppName {
        AppName::parse(text).expect("name")
    }

    #[test]
    fn only_the_shell_name_gives_a_role() {
        let table = [
            (vec!["org.quire.Shell"], None, Some(VoiceRole::Shell)),
            (vec!["org.quire.Mailo"], None, None),
            (vec!["org.other.Thing"], None, None),
            (vec![], Some("org.quire.Shell"), Some(VoiceRole::Shell)),
            (vec![], Some("org.quire.Mailo"), None),
            (vec![], None, None),
            // A name decides over the cgroup: owning a name that is nobody makes it nobody.
            (vec!["org.other.Thing"], Some("org.quire.Shell"), None),
        ];
        for (names, scope, want) in table {
            let facts = Facts {
                names: names.into_iter().map(name).collect(),
                scope: scope.map(name),
            };
            assert_eq!(derive(&config(), &facts), want, "{facts:?}");
        }
    }
}
