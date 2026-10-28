//! Where a conversation was started from, when it is not the opener's own app: the terminal a
//! `quire-do ask` came from. companiond opens that conversation, so the router's record of the
//! opener names companiond; this says which terminal the person was typing in.

use crate::caller::is_terminal_scope;
use serde::{Deserialize, Serialize};

/// The most characters a scope name holds (a systemd unit name's own limit).
const LONGEST: usize = 255;

/// The systemd scope of a terminal's child (`vte-spawn-*.scope`, `tmux-spawn-*.scope`,
/// `session-*.scope`): the leaf of its cgroup path, as `is_terminal_scope` accepts it. Advisory:
/// every terminal runs as the person, so the name tells which one, never who.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TerminalScope(String);

/// Why a name is not a terminal scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not the scope of a terminal's child")]
pub struct NotATerminalScope;

impl TerminalScope {
    /// The scope a cgroup path ends in, if it is a terminal's.
    pub fn from_cgroup(cgroup: &str) -> Result<Self, NotATerminalScope> {
        let leaf = cgroup.rsplit('/').next().unwrap_or_default();
        Self::try_from(leaf.to_owned())
    }

    /// The scope's name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for TerminalScope {
    type Error = NotATerminalScope;

    fn try_from(name: String) -> Result<Self, Self::Error> {
        let plain = name.len() <= LONGEST && name.chars().all(|c| c.is_ascii_graphic());
        if plain && !name.contains('/') && is_terminal_scope(&name) {
            Ok(Self(name))
        } else {
            Err(NotATerminalScope)
        }
    }
}

impl From<TerminalScope> for String {
    fn from(scope: TerminalScope) -> String {
        scope.0
    }
}

/// What started a conversation that another party opened on its behalf.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum StartedFrom {
    /// A terminal: `quire-do ask`, opened through the companion.
    Terminal(TerminalScope),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_terminal_childs_scope_is_one() {
        let table = [
            ("/user.slice/app.slice/vte-spawn-1.scope", true),
            ("tmux-spawn-2.scope", true),
            ("/x/session-3.scope", true),
            ("/x/app-org.quire.Mail-1.scope", false),
            ("/x/companiond.service", false),
            ("", false),
        ];
        for (path, ok) in table {
            assert_eq!(TerminalScope::from_cgroup(path).is_ok(), ok, "{path}");
        }
    }

    #[test]
    fn a_scope_read_from_a_log_is_checked() {
        let good: Result<StartedFrom, _> =
            serde_json::from_str(r#"{"kind":"terminal","v":"vte-spawn-1.scope"}"#);
        assert!(good.is_ok());
        let bad: Result<StartedFrom, _> =
            serde_json::from_str(r#"{"kind":"terminal","v":"../../etc"}"#);
        assert!(bad.is_err());
    }
}
