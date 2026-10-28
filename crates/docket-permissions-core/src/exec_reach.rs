//! Can a command send data out of the machine? (acp-sessions.md section 12, R11.) A command that
//! can always asks and is never granted, whether or not anything was read. The answer is the
//! sandbox's network plus a table of known network programs, shipped as `dist/network-programs.toml`
//! and passed in as [`NetPrograms`]; nothing here runs a command or reads a file.
//!
//! The table is a belt, not the lock: the terminal sandbox has no network today, so a program in
//! the table is stopped by the sandbox as well. It is the rule that holds when a sandbox is given
//! a network, and what keeps a grant for such a program from ever existing. It cannot see what a
//! script or an interpreter does with a socket, which is why `NetAccess::Open` makes every command
//! `Possible`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// What network the sandbox the command runs in has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetAccess {
    /// No network at all (`NetworkMode::None`).
    #[default]
    Closed,
    /// Anything else: the model endpoint only, or the host's network.
    Open,
}

impl NetAccess {
    /// The network a command really has when it runs under two sandboxes' worth of network (its
    /// own, and the agent process's that asked for it, R12): open when either is open.
    pub const fn combine(self, other: NetAccess) -> NetAccess {
        match (self, other) {
            (NetAccess::Closed, NetAccess::Closed) => NetAccess::Closed,
            _ => NetAccess::Open,
        }
    }
}

/// Whether the command can send data out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetReach {
    /// It cannot, as far as the program and the sandbox say.
    None,
    /// It can (or might): it always asks and is never granted.
    Possible,
}

/// The programs that reach the network, by the basename of the word that names them: the shipped
/// `dist/network-programs.toml`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetPrograms {
    /// Network tools: running one at all reaches out.
    pub any: Vec<String>,
    /// Programs that run other programs from their arguments: a network tool named anywhere in
    /// the line is as good as run.
    pub wrappers: Vec<String>,
    /// Programs that reach the network only for these subcommands.
    pub subcommands: BTreeMap<String, Vec<String>>,
}

impl NetPrograms {
    fn reaches(&self, token: &str, after: &[&str]) -> bool {
        let name = base(token);
        self.any.iter().any(|p| p == name)
            || self
                .subcommands
                .get(name)
                .is_some_and(|list| after.iter().any(|t| list.iter().any(|s| s == t)))
    }

    fn wraps(&self, token: &str) -> bool {
        self.wrappers.iter().any(|w| w == base(token))
    }
}

/// The words of `line` that could be a program or a subcommand: runs of letters, digits and
/// `. _ + - /`, so quotes, operators and `=` split words.
fn tokens(line: &str) -> Vec<&str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || "._+-/".contains(c)))
        .filter(|t| !t.is_empty())
        .collect()
}

fn base(token: &str) -> &str {
    token.rsplit('/').next().unwrap_or(token)
}

/// Whether a line is only plain words (no operator, quote or expansion): a shell would run
/// exactly one program.
fn single_program(line: &str) -> bool {
    crate::standing::CommandPrefix::parse(line).is_ok()
}

impl NetReach {
    /// Whether `line` (a command and its arguments) can send data out when it runs in a sandbox
    /// with `access`. With network open everything can. Otherwise the program is looked up by the
    /// basename of its first word; a subcommand program (`git`, `npm`) reaches when any later
    /// word names one of its network subcommands (so `git -C dir push` does too). A wrapper
    /// (`sh -c`, `env`, `xargs`) or a line with a shell operator is searched word by word, so a
    /// network tool anywhere in it counts. These false positives are on purpose: a word that only
    /// looks like a network tool asks, which is the safe side.
    pub fn of(line: &str, access: NetAccess, programs: &NetPrograms) -> NetReach {
        if access == NetAccess::Open {
            return NetReach::Possible;
        }
        let words = tokens(line);
        let search_all = !single_program(line) || words.first().is_some_and(|w| programs.wraps(w));
        let heads = if search_all {
            words.len()
        } else {
            1.min(words.len())
        };
        let hit = (0..heads).any(|i| programs.reaches(words[i], &words[i + 1..]));
        if hit {
            NetReach::Possible
        } else {
            NetReach::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn programs() -> NetPrograms {
        let list = |words: &[&str]| words.iter().map(|w| (*w).to_owned()).collect::<Vec<_>>();
        NetPrograms {
            any: list(&["curl", "nc"]),
            wrappers: list(&["sh", "env"]),
            subcommands: BTreeMap::from([("git".to_owned(), list(&["push", "pull"]))]),
        }
    }

    #[test]
    fn a_program_reaches_by_its_name_or_its_subcommand() {
        use NetReach::{None as No, Possible as Yes};
        let cases = [
            ("curl https://a.test/x", Yes),
            ("/usr/bin/curl -s a.test", Yes),
            ("git push origin main", Yes),
            ("git -C /home/u/proj push", Yes),
            ("git status", No),
            ("ls -la", No),
            ("sh -c 'curl a.test'", Yes),
            ("env FOO=1 nc a.test", Yes),
            ("echo hi; curl a.test", Yes),
            ("ls | nc host 9", Yes),
            ("cat curl.txt", No),
            ("echo $(curl h)", Yes),
        ];
        let programs = programs();
        for (line, want) in cases {
            assert_eq!(
                NetReach::of(line, NetAccess::Closed, &programs),
                want,
                "{line}"
            );
        }
    }

    #[test]
    fn an_open_sandbox_reaches_with_every_command() {
        for line in ["ls", "cargo test", "pwd", "git status"] {
            assert_eq!(
                NetReach::of(line, NetAccess::Open, &programs()),
                NetReach::Possible
            );
        }
    }

    #[test]
    fn combine_is_open_when_either_is() {
        use NetAccess::{Closed, Open};
        let table = [
            (Closed, Closed, Closed),
            (Closed, Open, Open),
            (Open, Closed, Open),
            (Open, Open, Open),
        ];
        for (a, b, want) in table {
            assert_eq!(a.combine(b), want, "{a:?} {b:?}");
        }
    }

    #[test]
    fn an_empty_line_reaches_nothing() {
        assert_eq!(
            NetReach::of("", NetAccess::Closed, &programs()),
            NetReach::None
        );
    }
}
