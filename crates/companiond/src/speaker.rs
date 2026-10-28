//! Who speaks for the person. Two callers may: the shell (sill: the process porter's callers
//! table names as the shell's unit, `sill.service`, with the system's `sheet_host` role and the
//! app `companiond.toml` gives it), and a terminal (`quire-do ask`: a process in a terminal's scope
//! that nothing else names, run by the same user), which may only open, ask and close, stop its
//! own answer and read answers.
//!
//! The shell is the process the callers table names, never the owner of a bus name: a scope that
//! claims the shell's app is a plain app, and a plain app is refused. The terminal is identified
//! the way intentd identifies the cli role (`docket_core::is_terminal_scope` over the cgroup
//! behind the pid the bus names for the connection) and only when the table names nothing and the
//! connection owns no name of its own; it is
//! the person at a keyboard, or whatever is typing in one, and that is why it gets no more than
//! the conversation: it cannot press a card (`Act`) and cannot tell a subagent anything (`Told`).
//! A third caller reads the helpers and speaks in a helper's conversation, and does nothing
//! else: our own editor bridge (`docket-acp`, the
//! process the table names as the app `org.quire.Acp` in the role `agent`, by its scope and its
//! program path), so an editor can show the helpers the person keeps. It is identified by the
//! table alone, never by a name it sends or a bus name it owns.
//! Every call the companion then makes goes through intentd's gate in the role `companion`, and
//! a confirmation is still answered on the sheet by the person.

use companion_core::{Call, Speaker, permits};
use docket_core::TerminalScope;
use docket_dbus::BusConnection;
use docket_dbus::callers::{Caller, Callers, Identity, Role};
use docket_procroot::{own_uid, terminal_scope};
use porter_core::AppName;
use porter_daemon::ProcRoot;
use std::sync::Arc;
use zbus::fdo::{self, DBusProxy};
use zbus::message::Header;
use zbus::names::BusName;

/// The app the table names for our own editor bridge.
pub(crate) const EDITOR_APP: &str = "org.quire.Acp";

fn denied() -> fdo::Error {
    fdo::Error::AccessDenied("only the shell or a terminal speaks for the person".into())
}

async fn owns_a_name(dbus: &DBusProxy<'_>, unique: &str) -> fdo::Result<bool> {
    for name in dbus.list_names().await? {
        let text = name.as_str();
        if text.starts_with(':') {
            continue;
        }
        let Ok(bus) = BusName::try_from(text) else {
            continue;
        };
        if dbus
            .get_name_owner(bus)
            .await
            .is_ok_and(|o| o.as_str() == unique)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Who the process is that the callers table names as `named` (or does not name, `None`) and whose
/// cgroup is `terminal` (if it is a terminal's child): the shell is the table's `sheet_host` of the
/// app `shell`; the editor bridge is the table's `agent` of the app `org.quire.Acp`; a process the table names as anything else is nobody here; a process the table
/// does not name is a terminal when its cgroup says so.
pub(crate) fn speaker_of(
    shell: &AppName,
    named: Option<&Caller>,
    terminal: Option<TerminalScope>,
) -> Option<Speaker> {
    match named {
        Some(caller) if caller.role == Role::SheetHost && &caller.app.name == shell => {
            Some(Speaker::Shell)
        }
        Some(caller) if caller.role == Role::Agent && caller.app.name.as_str() == EDITOR_APP => {
            Some(Speaker::Editor)
        }
        Some(_) => None,
        None => terminal.map(Speaker::Terminal),
    }
}

/// How the daemon tells who is on the other end of a call: the callers table through `who`, the
/// terminals' cgroups under `proc_root`.
#[derive(Debug, Clone)]
pub struct Speakers {
    connection: BusConnection,
    who: Arc<Identity>,
    shell: AppName,
    proc_root: ProcRoot,
}

impl Speakers {
    /// Speakers on `connection`: `shell` is the app that speaks as the shell when `who` names it
    /// so, and `proc_root` is where a terminal's cgroup is read (`/proc`, or a test's tree).
    pub fn new(
        connection: BusConnection,
        who: Arc<Identity>,
        shell: AppName,
        proc_root: ProcRoot,
    ) -> Self {
        Self {
            connection,
            who,
            shell,
            proc_root,
        }
    }

    /// The terminal scope of the connection `sender`, if the same user's process behind it is a
    /// terminal's child.
    async fn terminal(&self, sender: &str) -> fdo::Result<Option<TerminalScope>> {
        let dbus = DBusProxy::new(&self.connection).await?;
        let who = BusName::try_from(sender).map_err(|_| denied())?;
        let credentials = dbus
            .get_connection_credentials(who)
            .await
            .map_err(|_| denied())?;
        let same_user = own_uid().is_some_and(|ours| credentials.unix_user_id() == Some(ours));
        let scope = credentials
            .process_id()
            .filter(|_| same_user)
            .and_then(|pid| terminal_scope(self.proc_root.path(), pid));
        // Narrowing, not identification: a terminal's child that owns a name of its own is an
        // application the person started there, not the person's keyboard.
        match scope {
            Some(_) if owns_a_name(&dbus, sender).await? => Ok(None),
            other => Ok(other),
        }
    }

    /// Who is on the other end of this call: the shell if the table names the sender so, a
    /// terminal if nothing names it and it is the same user in a terminal's scope, else nobody.
    pub async fn identify(&self, header: &Header<'_>) -> fdo::Result<Speaker> {
        let sender = header.sender().map(|s| s.as_str()).ok_or_else(denied)?;
        let named = self.who.caller_of(sender).await;
        let terminal = match named {
            Some(_) => None,
            None => self.terminal(sender).await?,
        };
        speaker_of(&self.shell, named.as_ref(), terminal).ok_or_else(denied)
    }

    /// [`Speakers::identify`], then the table: the call is refused for a speaker that may not
    /// make it.
    pub async fn require(&self, header: &Header<'_>, call: Call) -> fdo::Result<Speaker> {
        let speaker = self.identify(header).await?;
        if permits(&speaker, call) {
            Ok(speaker)
        } else {
            Err(fdo::Error::AccessDenied(match speaker {
                Speaker::Editor => "an editor may look at the helpers and speak in their conversations, nothing else".into(),
                _ => "a terminal may open, ask, close, cancel and read, nothing else".into(),
            }))
        }
    }

    /// [`Speakers::require`] for a property read, where the bus may hand no header: that caller
    /// is nobody.
    pub async fn require_read(&self, header: Option<&Header<'_>>) -> fdo::Result<Speaker> {
        let header = header.ok_or_else(denied)?;
        self.require(header, Call::Read).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use docket_dbus::callers::{CallerTable, ProcCallers, table_from_toml};
    use std::path::Path;

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

    /// Who the process is, by the table and the fake proc root, as `Speakers::identify` reads it.
    fn speaker(root: &Path, pid: u32, table: &CallerTable) -> Option<Speaker> {
        let shell = AppName::parse("org.quire.Shell").expect("app");
        let named = ProcCallers::caller_of_pid(root, pid, table);
        let terminal = match named {
            Some(_) => None,
            None => terminal_scope(root, pid),
        };
        speaker_of(&shell, named.as_ref(), terminal)
    }

    #[test]
    fn only_the_shells_unit_is_the_shell_and_only_an_unnamed_terminal_child_is_a_terminal() {
        let root = tempfile::tempdir().expect("scratch");
        let root = root.path();
        let table = crate::callers::shipped();
        for (pid, leaf, main) in [
            (1, "sill.service", true),
            (2, "sill.service", false),
            (3, "sill-shell.scope", false),
            (4, "app-org.quire.Shell-1.scope", false),
            (5, "vte-spawn-abc.scope", false),
            (6, "tmux-spawn-abc.scope", false),
            (7, "companiond.service", true),
            (8, "app-org.quire.Mail-1.scope", false),
        ] {
            put(root, pid, leaf, main);
        }
        let term = |leaf: &str| {
            Some(Speaker::Terminal(
                TerminalScope::from_cgroup(leaf).expect("scope"),
            ))
        };
        let rows = [
            ("the shell's unit", 1, Some(Speaker::Shell)),
            (
                "a process of the shell's unit that is not its main process",
                2,
                None,
            ),
            (
                "the shell's old scope, which the table does not name",
                3,
                None,
            ),
            ("an app scope called like the shell", 4, None),
            ("a vte terminal's child", 5, term("vte-spawn-abc.scope")),
            ("a tmux terminal's child", 6, term("tmux-spawn-abc.scope")),
            ("a unit the table does not name", 7, None),
            ("a plain app", 8, None),
            ("no such process", 99, None),
        ];
        for (what, pid, expected) in rows {
            assert_eq!(speaker(root, pid, &table), expected, "{what}");
        }
    }

    #[test]
    fn a_user_row_cannot_make_a_process_the_shell_and_a_lowered_unit_is_not() {
        let user = table_from_toml(
            "[[caller]]\napp = \"org.quire.Shell\"\nunit = \"mine.service\"\nrole = \"sheet_host\"\n\n\
             [[caller]]\napp = \"org.quire.Shell\"\nunit = \"sill.service\"\nrole = \"agent\"\n",
        )
        .expect("user");
        let table = CallerTable::layered(crate::callers::shipped(), user);
        let root = tempfile::tempdir().expect("scratch");
        put(root.path(), 1, "mine.service", true);
        put(root.path(), 2, "sill.service", true);
        assert_eq!(speaker(root.path(), 1, &table), None);
        assert_eq!(speaker(root.path(), 2, &table), None);
    }
}
