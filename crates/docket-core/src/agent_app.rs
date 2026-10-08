//! The pseudo-app an external coding agent's calls are made in: `org.quire.AcpAgent`. The host
//! that launched the agent (`docket-agent`) makes every call the agent asks of it (a file read or
//! write, a command, a permission request) as an ordinary router call of one of these actions,
//! and answers as the router rules; it performs a call only after the router has allowed it.
//!
//! The names live here because the host, the router and the manifest all use them. The actions'
//! declarations are `manifests/org.quire.AcpAgent.toml`.

use crate::ids::ActionRef;
use crate::standing_match::{ArgFacts, CallFacts};
use porter_core::AppName;
use prov::{ActionName, Effect};

/// The pseudo-app and the bus name of the host that serves its actions.
pub const ACP_AGENT_APP: &str = "org.quire.AcpAgent";

/// `fs/read_text_file`.
pub const FILES_READ: &str = "acpagent.files.read";
/// `fs/write_text_file`.
pub const FILES_WRITE: &str = "acpagent.files.write";
/// `fs/write_text_file` to a place that later runs code outside the sandbox (`.git`, an editor or
/// agent configuration, a shell rc file): it asks every time and never offers "always".
pub const FILES_SENSITIVE: &str = "acpagent.files.sensitive";
/// `terminal/create`.
pub const TERMINAL_RUN: &str = "acpagent.terminal.run";
/// A tool call the agent only reports having made itself.
pub const REPORTED: &str = "acpagent.reported";

/// What an agent's permission request names it wants to do (ACP's tool kinds). Each is an
/// action of its own, `acpagent.<kind>`; the host answers the agent from the router's ruling.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PermissionKind {
    /// Read a file.
    Read,
    /// Edit a file.
    Edit,
    /// Delete a file.
    Delete,
    /// Move a file.
    Move,
    /// Search.
    Search,
    /// Run a command.
    Execute,
    /// Think.
    Think,
    /// Fetch from the network.
    Fetch,
    /// Switch its own mode.
    SwitchMode,
    /// Anything else, and anything unclassified.
    Other,
}

impl PermissionKind {
    /// Every kind.
    pub const ALL: [PermissionKind; 10] = [
        PermissionKind::Read,
        PermissionKind::Edit,
        PermissionKind::Delete,
        PermissionKind::Move,
        PermissionKind::Search,
        PermissionKind::Execute,
        PermissionKind::Think,
        PermissionKind::Fetch,
        PermissionKind::SwitchMode,
        PermissionKind::Other,
    ];

    /// The kind's word.
    pub fn word(self) -> &'static str {
        match self {
            PermissionKind::Read => "read",
            PermissionKind::Edit => "edit",
            PermissionKind::Delete => "delete",
            PermissionKind::Move => "move",
            PermissionKind::Search => "search",
            PermissionKind::Execute => "execute",
            PermissionKind::Think => "think",
            PermissionKind::Fetch => "fetch",
            PermissionKind::SwitchMode => "switch_mode",
            PermissionKind::Other => "other",
        }
    }

    /// The action name: `acpagent.<kind>`.
    pub fn action_name(self) -> String {
        format!("acpagent.{}", self.word())
    }

    /// The effect it is gated as: reads read; edit and move write (undoable, the host keeps the
    /// old text); execute and fetch go out (`EXECUTE_AS`); delete, a mode switch and anything
    /// unclassified are destructive until classified (they never offer "always").
    pub fn effect(self) -> Effect {
        match self {
            PermissionKind::Read | PermissionKind::Search | PermissionKind::Think => Effect::Read,
            PermissionKind::Edit | PermissionKind::Move => Effect::UndoableWrite,
            PermissionKind::Execute | PermissionKind::Fetch => Effect::Outbound,
            PermissionKind::Delete | PermissionKind::SwitchMode | PermissionKind::Other => {
                Effect::Destructive
            }
        }
    }
}

/// The pseudo-app's name.
pub fn app() -> Option<AppName> {
    AppName::parse(ACP_AGENT_APP).ok()
}

/// `name` in the pseudo-app.
pub fn action(name: &str) -> Option<ActionRef> {
    Some(ActionRef {
        app: app()?,
        name: ActionName::parse(name).ok()?,
    })
}

/// Whether `action` is one of the pseudo-app's.
pub fn is_agent_action(action: &ActionRef) -> bool {
    action.app.as_str() == ACP_AGENT_APP
}

/// The call a performed permission request approves, once: an edit or a move approves the next
/// write of those files; an execute approves the next command of that line in that directory. A
/// permission request is the person's yes to the thing it names, so the matching call does not
/// ask again; every other check still runs on it. Nothing else approves anything.
pub fn approves(permission: &CallFacts) -> Option<CallFacts> {
    if !is_agent_action(&permission.action) {
        return None;
    }
    let (target, args) = match (permission.action.name.as_str(), &permission.args) {
        (n, ArgFacts::Paths(_))
            if n == PermissionKind::Edit.action_name()
                || n == PermissionKind::Move.action_name() =>
        {
            (FILES_WRITE, permission.args.clone())
        }
        (n, ArgFacts::Command { .. }) if n == PermissionKind::Execute.action_name() => {
            (TERMINAL_RUN, permission.args.clone())
        }
        _ => return None,
    };
    Some(CallFacts {
        action: action(target)?,
        args,
    })
}

/// Whether the approval `held` covers a call whose facts are `call`: the same action, every path
/// of the call among the approved ones, or the same command line in the same directory.
pub fn covers_approval(held: &CallFacts, call: &CallFacts) -> bool {
    if held.action != call.action {
        return false;
    }
    match (&held.args, &call.args) {
        (ArgFacts::Paths(approved), ArgFacts::Paths(wanted)) => {
            !wanted.is_empty() && wanted.iter().all(|p| approved.contains(p))
        }
        (a @ ArgFacts::Command { .. }, b @ ArgFacts::Command { .. }) => a == b,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::standing::AbsPath;

    fn path(text: &str) -> AbsPath {
        AbsPath::parse(text).expect("path")
    }

    fn facts(name: &str, args: ArgFacts) -> CallFacts {
        CallFacts {
            action: action(name).expect("action"),
            args,
        }
    }

    #[test]
    fn an_edit_approves_the_write_of_the_same_files_and_nothing_else() {
        let edit = facts(
            "acpagent.edit",
            ArgFacts::Paths(vec![path("/home/u/p/a.rs")]),
        );
        let held = approves(&edit).expect("approves a write");
        assert_eq!(held.action.name.as_str(), FILES_WRITE);
        let same = facts(FILES_WRITE, ArgFacts::Paths(vec![path("/home/u/p/a.rs")]));
        let other = facts(FILES_WRITE, ArgFacts::Paths(vec![path("/home/u/p/b.rs")]));
        let both = facts(
            FILES_WRITE,
            ArgFacts::Paths(vec![path("/home/u/p/a.rs"), path("/home/u/p/b.rs")]),
        );
        assert!(covers_approval(&held, &same));
        assert!(!covers_approval(&held, &other));
        assert!(!covers_approval(&held, &both));
        assert!(approves(&facts("acpagent.delete", edit.args.clone())).is_none());
        assert!(approves(&facts("acpagent.fetch", ArgFacts::Unscoped)).is_none());
    }

    #[test]
    fn an_execute_approves_the_same_line_in_the_same_directory() {
        let run = ArgFacts::Command {
            line: "cargo test".into(),
            cwd: path("/home/u/p"),
        };
        let held = approves(&facts("acpagent.execute", run.clone())).expect("approves a command");
        assert!(covers_approval(&held, &facts(TERMINAL_RUN, run)));
        let elsewhere = ArgFacts::Command {
            line: "cargo test".into(),
            cwd: path("/home/u/q"),
        };
        assert!(!covers_approval(&held, &facts(TERMINAL_RUN, elsewhere)));
        let other = ArgFacts::Command {
            line: "cargo test; rm -rf /".into(),
            cwd: path("/home/u/p"),
        };
        assert!(!covers_approval(&held, &facts(TERMINAL_RUN, other)));
    }

    #[test]
    fn every_kind_has_a_valid_action_name_and_the_unclassified_are_destructive() {
        for kind in PermissionKind::ALL {
            assert!(ActionName::parse(&kind.action_name()).is_ok(), "{kind:?}");
        }
        assert_eq!(PermissionKind::Other.effect(), Effect::Destructive);
        assert_eq!(PermissionKind::SwitchMode.effect(), Effect::Destructive);
        assert_eq!(PermissionKind::Execute.effect(), Effect::Outbound);
        assert_eq!(PermissionKind::Read.effect(), Effect::Read);
    }
}
