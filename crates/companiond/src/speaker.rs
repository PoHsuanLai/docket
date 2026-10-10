//! Who speaks for the person. Two callers may: the shell (sill, the owner of the bus name
//! `companiond.toml` gives it), and a terminal (`quire-do ask`: a process in a terminal's scope
//! that owns no bus name, run by the same user), which may only open, ask and close.
//!
//! The terminal is identified the way intentd identifies the cli role (`docket_core::
//! is_terminal_scope` over the cgroup behind the pid the bus names for the connection); it is
//! the person at a keyboard, or whatever is typing in one, and that is why it gets no more than
//! the three calls: it cannot press a card (`Act`) and cannot tell a subagent anything (`Told`).
//! Every call the companion then makes goes through intentd's gate in the role `companion`, and
//! a confirmation is still answered on the sheet by the person.

use docket_core::TerminalScope;
use docket_dbus::BusConnection;
use porter_core::{AppName, CgroupPath};
use porter_daemon::ProcGate;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use zbus::fdo::{self, DBusProxy};
use zbus::message::Header;
use zbus::names::BusName;

/// The variable that moves the `/proc` the daemon reads, in a build with `test-proc-root`.
pub const PROC_ROOT_VAR: &str = "COMPANIOND_PROC_ROOT";

/// Whether this build honours [`PROC_ROOT_VAR`]: only one built with the `test-proc-root`
/// feature. In a normal build the variable is ignored (and said so on stderr), so an environment
/// cannot make a production daemon believe a caller is a terminal.
pub const PROC_GATE: ProcGate = if cfg!(feature = "test-proc-root") {
    ProcGate::Honour
} else {
    ProcGate::Ignore
};

/// Who a caller is, as far as the person's voice goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Speaker {
    /// The shell.
    Shell,
    /// A terminal's child, in this scope.
    Terminal(TerminalScope),
}

/// What a caller asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Call {
    /// `Open`.
    Open,
    /// `Ask`.
    Ask,
    /// `Close`.
    Close,
    /// `Told`: words to a subagent.
    Told,
    /// `Act`: presses a card.
    Act,
}

/// Whether `speaker` may make `call`: the shell anything, a terminal only the conversation.
pub fn permits(speaker: &Speaker, call: Call) -> bool {
    match speaker {
        Speaker::Shell => true,
        Speaker::Terminal(_) => matches!(call, Call::Open | Call::Ask | Call::Close),
    }
}

fn denied() -> fdo::Error {
    fdo::Error::AccessDenied("only the shell or a terminal speaks for the person".into())
}

/// The terminal scope the cgroup file of `pid` under `root` names, if it is a terminal's child.
fn in_terminal(root: &Path, pid: u32) -> Option<TerminalScope> {
    let text =
        std::fs::read_to_string(root.join(pid.to_string()).join("cgroup")).unwrap_or_default();
    let cgroup = CgroupPath::from_proc_cgroup(&text).ok()?;
    TerminalScope::from_cgroup(cgroup.as_str()).ok()
}

fn own_uid() -> Option<u32> {
    std::fs::metadata("/proc/self").ok().map(|m| m.uid())
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

/// Who is on the other end of this call: the shell if it owns the shell's name, a terminal if
/// it is the same user in a terminal's scope owning no name of its own, else nobody.
pub async fn identify(
    connection: &BusConnection,
    shell: &AppName,
    root: &Path,
    header: &Header<'_>,
) -> fdo::Result<Speaker> {
    let sender = header.sender().map(ToString::to_string).unwrap_or_default();
    let dbus = DBusProxy::new(connection).await?;
    let name = BusName::try_from(shell.as_str()).map_err(|e| fdo::Error::Failed(e.to_string()))?;
    if let Ok(owner) = dbus.get_name_owner(name).await
        && owner.as_str() == sender
    {
        return Ok(Speaker::Shell);
    }
    let who = BusName::try_from(sender.as_str()).map_err(|_| denied())?;
    let credentials = dbus
        .get_connection_credentials(who)
        .await
        .map_err(|_| denied())?;
    let same_user = own_uid().is_some_and(|ours| credentials.unix_user_id() == Some(ours));
    let terminal = credentials
        .process_id()
        .and_then(|pid| in_terminal(root, pid));
    match terminal {
        Some(scope) if same_user && !owns_a_name(&dbus, &sender).await? => {
            Ok(Speaker::Terminal(scope))
        }
        _ => Err(denied()),
    }
}

/// [`identify`], then the table: the call is refused for a speaker that may not make it.
pub async fn require(
    connection: &BusConnection,
    shell: &AppName,
    root: &Path,
    header: &Header<'_>,
    call: Call,
) -> fdo::Result<Speaker> {
    let speaker = identify(connection, shell, root, header).await?;
    if permits(&speaker, call) {
        Ok(speaker)
    } else {
        Err(fdo::Error::AccessDenied(
            "a terminal may open, ask and close, nothing else".into(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> TerminalScope {
        TerminalScope::from_cgroup("vte-spawn-1.scope").expect("scope")
    }

    #[test]
    fn the_shell_may_do_everything_and_a_terminal_only_the_conversation() {
        let all = [Call::Open, Call::Ask, Call::Close, Call::Told, Call::Act];
        let terminal: Vec<Call> = all
            .into_iter()
            .filter(|c| permits(&Speaker::Terminal(scope()), *c))
            .collect();
        assert_eq!(terminal, [Call::Open, Call::Ask, Call::Close]);
        assert!(all.into_iter().all(|c| permits(&Speaker::Shell, c)));
    }

    #[test]
    fn only_a_terminal_scope_is_a_terminal() {
        let dir = tempfile::tempdir().expect("scratch");
        let place = |pid: u32, leaf: &str| {
            let at = dir.path().join(pid.to_string());
            std::fs::create_dir_all(&at).expect("dir");
            std::fs::write(
                at.join("cgroup"),
                format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{leaf}\n"),
            )
            .expect("cgroup");
        };
        place(1, "vte-spawn-abc.scope");
        place(2, "tmux-spawn-abc.scope");
        place(3, "companiond.service");
        place(4, "app-org.quire.Mail-1.scope");
        let rows = [(1, true), (2, true), (3, false), (4, false), (99, false)];
        for (pid, expected) in rows {
            assert_eq!(
                in_terminal(dir.path(), pid).is_some(),
                expected,
                "pid {pid}"
            );
        }
    }

    #[test]
    fn the_proc_gate_follows_the_test_proc_root_feature() {
        let expected = if cfg!(feature = "test-proc-root") {
            ProcGate::Honour
        } else {
            ProcGate::Ignore
        };
        assert_eq!(PROC_GATE, expected);
    }
}
