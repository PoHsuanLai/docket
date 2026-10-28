//! Who is on the other end of a bus connection. The identity is derived from what the bus and
//! `/proc` say about the process behind the connection and never from anything the caller sends,
//! and never from a name it owns: porter's `ProcCallers` (through [`Identity`]) names the unit or
//! app scope (the bus gives the pid, `/proc/<pid>/cgroup` names the unit or app, `voiced-callers.toml`
//! gives the role). The roles are `voiced.toml`'s: the shell is the app that file lists as `shell`
//! when the callers table gives it the system's `sheet_host` role, which only a unit's main
//! process has (a scope claiming the app is a plain app). An app that an utterance is routed to
//! attaches as the app its scope or Flatpak metadata names. This is advisory on a desktop where
//! every process runs as the person: what a process cannot do is be more than the role its unit
//! plays, and only the shell begins.

use crate::config::{VoiceRole, VoicedConfig};
use docket_dbus::callers::{Caller, Callers, Identity, Role};
use docket_procroot::own_uid;
use porter_core::AppName;
use std::sync::Arc;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

/// The role `caller` has: the shell when the table gives it `sheet_host` and the configuration
/// lists its app as the shell. `App` is never derived: it is whoever a route names.
pub(crate) fn derive(config: &VoicedConfig, caller: &Caller) -> Option<VoiceRole> {
    (caller.role == Role::SheetHost && config.role_of(&caller.app.name) == Some(VoiceRole::Shell))
        .then_some(VoiceRole::Shell)
}

/// Derives the roles of the connections on one bus.
#[derive(Debug, Clone)]
pub struct Peers {
    connection: zbus::Connection,
    config: Arc<VoicedConfig>,
    who: Arc<Identity>,
}

impl Peers {
    /// Roles on `connection` by `config`, naming the processes through `who`.
    pub fn new(
        connection: zbus::Connection,
        config: Arc<VoicedConfig>,
        who: Arc<Identity>,
    ) -> Self {
        Self {
            connection,
            config,
            who,
        }
    }

    /// Whether the connection `unique` is the same user's: a session bus is one user's, and a
    /// connection of another is nobody here.
    async fn ours(&self, unique: &str) -> bool {
        let Ok(dbus) = DBusProxy::new(&self.connection).await else {
            return false;
        };
        let Ok(name) = BusName::try_from(unique) else {
            return false;
        };
        let Ok(credentials) = dbus.get_connection_credentials(name).await else {
            return false;
        };
        own_uid().is_none_or(|ours| credentials.unix_user_id() == Some(ours))
    }

    /// The caller behind the connection `unique`: none for a stranger, another user's, or one
    /// that vanished.
    async fn caller(&self, unique: &str) -> Option<Caller> {
        if !self.ours(unique).await {
            return None;
        }
        self.who.caller_of(unique).await
    }

    /// The role of the connection `unique`: none for a stranger, another user's, or one that
    /// vanished.
    pub async fn role(&self, unique: &str) -> Option<VoiceRole> {
        let caller = self.caller(unique).await?;
        derive(&self.config, &caller)
    }

    /// Whether the connection `unique` is the app `name`: its process is identified as that app.
    pub async fn is_app(&self, unique: &str, name: &AppName) -> bool {
        self.caller(unique)
            .await
            .is_some_and(|caller| &caller.app.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_dbus::callers::{CallerTable, ProcCallers, table_from_toml};
    use std::path::Path;

    fn config() -> VoicedConfig {
        VoicedConfig::parse(
            "earcons = \"on\"\n[roles]\nshell = [\"org.quire.Shell\"]\napp = [\"org.quire.Mailo\"]\n",
        )
        .expect("config")
    }

    /// Process `pid` in the fake proc root, in the cgroup `leaf`; a service's process is also its
    /// unit's main process when `main` says so.
    fn put(root: &Path, pid: u32, leaf: &str, main: bool) {
        let at = root.join(pid.to_string());
        std::fs::create_dir_all(&at).expect("dir");
        std::fs::write(
            at.join("cgroup"),
            format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{leaf}\n"),
        )
        .expect("cgroup");
        if main {
            std::fs::create_dir_all(root.join("units")).expect("units");
            std::fs::write(root.join("units").join(leaf), pid.to_string()).expect("main pid");
        }
    }

    fn role_of(root: &Path, pid: u32, table: &CallerTable) -> Option<VoiceRole> {
        ProcCallers::caller_of_pid(root, pid, table).and_then(|c| derive(&config(), &c))
    }

    #[test]
    fn only_the_shells_unit_is_the_shell() {
        let root = tempfile::tempdir().expect("scratch");
        let root = root.path();
        let table = crate::callers::shipped();
        for (pid, leaf, main) in [
            (1, "sill.service", true),
            (2, "sill.service", false),
            (3, "sill-shell.scope", false),
            (4, "app-org.quire.Shell-1.scope", false),
            (5, "app-org.quire.Mailo-1.scope", false),
            (6, "curl.service", true),
            (7, "vte-spawn-1.scope", false),
        ] {
            put(root, pid, leaf, main);
        }
        let rows = [
            ("the shell's unit", 1, Some(VoiceRole::Shell)),
            (
                "a process of the shell's unit that is not its main process",
                2,
                None,
            ),
            ("a scope the table does not name", 3, None),
            ("an app scope called like the shell", 4, None),
            ("the routed app", 5, None),
            ("a unit the table does not name", 6, None),
            ("a terminal's child", 7, None),
            ("no such process", 99, None),
        ];
        for (what, pid, expected) in rows {
            assert_eq!(role_of(root, pid, &table), expected, "{what}");
        }
    }

    #[test]
    fn a_users_row_cannot_make_an_app_the_shell() {
        let user = table_from_toml(
            "[[caller]]\napp = \"org.quire.Shell\"\nunit = \"mine.service\"\nrole = \"sheet_host\"\n",
        )
        .expect("user");
        let table = CallerTable::layered(crate::callers::shipped(), user);
        let root = tempfile::tempdir().expect("scratch");
        put(root.path(), 1, "mine.service", true);
        assert_eq!(role_of(root.path(), 1, &table), None);
    }

    #[test]
    fn a_shell_in_the_table_that_the_configuration_does_not_list_is_no_shell() {
        let table = table_from_toml(
            "[[caller]]\napp = \"org.example.Other\"\nunit = \"other.service\"\nrole = \"sheet_host\"\n",
        )
        .expect("table");
        let root = tempfile::tempdir().expect("scratch");
        put(root.path(), 1, "other.service", true);
        assert_eq!(role_of(root.path(), 1, &table), None);
    }
}
