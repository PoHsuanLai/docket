//! Can a command send data out of the machine? (acp-sessions.md section 12, R11.) A command that
//! can always asks and is never granted, whether or not anything was read. The answer is the
//! sandbox's network plus a const table of known network programs; nothing here runs a command or
//! reads a file.
//!
//! The table is a belt, not the lock: the terminal sandbox has no network today, so a program in
//! the table is stopped by the sandbox as well. It is the rule that holds when a sandbox is given
//! a network, and what keeps a grant for such a program from ever existing. It cannot see what a
//! script or an interpreter does with a socket, which is why `NetAccess::Open` makes every command
//! `Possible`.

use serde::{Deserialize, Serialize};

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

/// Which of a program's subcommands reach the network.
#[derive(Debug, Clone, Copy)]
enum Subs {
    /// The program itself is a network tool.
    Any,
    /// Only these subcommands are.
    Only(&'static [&'static str]),
}

/// Programs that reach the network, by the basename of the word that names them. `cargo build`,
/// `cargo test`, `cargo check` and `cargo run` are not listed: they may fetch crates from the
/// registry when the lockfile is not yet vendored, which sends out nothing the agent chose, and
/// the owner wants them grantable. `cargo install`, `publish` and the like are.
const PROGRAMS: &[(&str, Subs)] = &[
    ("curl", Subs::Any),
    ("wget", Subs::Any),
    ("aria2c", Subs::Any),
    ("http", Subs::Any),
    ("https", Subs::Any),
    ("nc", Subs::Any),
    ("ncat", Subs::Any),
    ("netcat", Subs::Any),
    ("socat", Subs::Any),
    ("telnet", Subs::Any),
    ("ftp", Subs::Any),
    ("sftp", Subs::Any),
    ("lftp", Subs::Any),
    ("ssh", Subs::Any),
    ("scp", Subs::Any),
    ("rsync", Subs::Any),
    ("mosh", Subs::Any),
    ("ping", Subs::Any),
    ("dig", Subs::Any),
    ("nslookup", Subs::Any),
    ("sendmail", Subs::Any),
    ("msmtp", Subs::Any),
    ("gh", Subs::Any),
    ("apt", Subs::Any),
    ("apt-get", Subs::Any),
    ("dnf", Subs::Any),
    ("yum", Subs::Any),
    ("pacman", Subs::Any),
    ("zypper", Subs::Any),
    ("brew", Subs::Any),
    ("flatpak", Subs::Any),
    ("snap", Subs::Any),
    (
        "git",
        Subs::Only(&[
            "push",
            "fetch",
            "clone",
            "pull",
            "ls-remote",
            "submodule",
            "remote",
            "lfs",
            "send-email",
        ]),
    ),
    (
        "cargo",
        Subs::Only(&[
            "publish", "install", "fetch", "update", "search", "login", "owner", "yank", "add",
        ]),
    ),
    (
        "npm",
        Subs::Only(&[
            "publish",
            "install",
            "i",
            "ci",
            "add",
            "update",
            "view",
            "login",
            "adduser",
            "audit",
            "access",
            "token",
            "unpublish",
            "deprecate",
            "dist-tag",
            "search",
            "outdated",
        ]),
    ),
    (
        "pnpm",
        Subs::Only(&["publish", "install", "i", "add", "update", "fetch", "login"]),
    ),
    (
        "yarn",
        Subs::Only(&["publish", "install", "add", "upgrade", "npm", "login"]),
    ),
    (
        "pip",
        Subs::Only(&["install", "download", "wheel", "index", "search"]),
    ),
    (
        "pip3",
        Subs::Only(&["install", "download", "wheel", "index", "search"]),
    ),
    (
        "uv",
        Subs::Only(&[
            "add", "sync", "lock", "publish", "pip", "tool", "python", "venv",
        ]),
    ),
    ("go", Subs::Only(&["get", "install", "mod"])),
    (
        "docker",
        Subs::Only(&["push", "pull", "login", "build", "run", "compose"]),
    ),
    (
        "podman",
        Subs::Only(&["push", "pull", "login", "build", "run"]),
    ),
    ("gem", Subs::Only(&["install", "push", "fetch", "update"])),
    ("bundle", Subs::Only(&["install", "update"])),
];

/// Programs that run other programs from their arguments: a network tool named anywhere in the
/// line is as good as run.
const WRAPPERS: &[&str] = &[
    "sh", "bash", "zsh", "dash", "fish", "env", "sudo", "doas", "xargs", "timeout", "nice",
    "nohup", "time", "exec", "command", "setsid", "stdbuf", "busybox", "watch", "script",
];

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

fn reaches(token: &str, after: &[&str]) -> bool {
    PROGRAMS
        .iter()
        .find(|(name, _)| *name == base(token))
        .is_some_and(|(_, subs)| match subs {
            Subs::Any => true,
            Subs::Only(list) => after.iter().any(|t| list.contains(t)),
        })
}

impl NetReach {
    /// Whether `line` (a command and its arguments) can send data out when it runs in a sandbox
    /// with `access`. With network open everything can. Otherwise the program is looked up by the
    /// basename of its first word; a subcommand program (`git`, `npm`) reaches when any later
    /// word names one of its network subcommands (so `git -C dir push` does too). A wrapper
    /// (`sh -c`, `env`, `xargs`) or a line with a shell operator is searched word by word, so a
    /// network tool anywhere in it counts. These false positives are on purpose: a word that only
    /// looks like a network tool asks, which is the safe side.
    pub fn of(line: &str, access: NetAccess) -> NetReach {
        if access == NetAccess::Open {
            return NetReach::Possible;
        }
        let words = tokens(line);
        let search_all =
            !single_program(line) || words.first().is_some_and(|w| WRAPPERS.contains(&base(w)));
        let heads = if search_all {
            words.len()
        } else {
            1.min(words.len())
        };
        let hit = (0..heads).any(|i| reaches(words[i], &words[i + 1..]));
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

    #[test]
    fn the_table_of_commands() {
        use NetReach::{None as No, Possible as Yes};
        let cases = [
            ("curl https://a.test/x", Yes),
            ("/usr/bin/curl -s a.test", Yes),
            ("wget a.test", Yes),
            ("ssh host ls", Yes),
            ("scp a host:b", Yes),
            ("rsync -a . host:/b", Yes),
            ("nc -l 9", Yes),
            ("git push origin main", Yes),
            ("git -C /home/u/proj push", Yes),
            ("git fetch", Yes),
            ("git clone x", Yes),
            ("git pull", Yes),
            ("git status", No),
            ("git diff HEAD~1", No),
            ("git log --oneline", No),
            ("cargo publish", Yes),
            ("cargo install ripgrep", Yes),
            ("cargo build", No),
            ("cargo test --lib", No),
            ("cargo clippy", No),
            ("npm publish", Yes),
            ("npm install left-pad", Yes),
            ("npm test", No),
            ("npm run build", No),
            ("pip install x", Yes),
            ("pip list", No),
            ("ls -la", No),
            ("cat Cargo.toml", No),
            ("echo hello", No),
            ("cd src", No),
            ("pwd", No),
            ("sh -c 'curl a.test'", Yes),
            ("env FOO=1 wget a.test", Yes),
            ("echo hi; curl a.test", Yes),
            ("ls | nc host 9", Yes),
            ("ls && cargo test", No),
            ("cat curl.txt", No),
            ("echo $(ssh h)", Yes),
        ];
        for (line, want) in cases {
            assert_eq!(NetReach::of(line, NetAccess::Closed), want, "{line}");
        }
    }

    #[test]
    fn an_open_sandbox_reaches_with_every_command() {
        for line in ["ls", "cargo test", "pwd", "git status"] {
            assert_eq!(NetReach::of(line, NetAccess::Open), NetReach::Possible);
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
        assert_eq!(NetReach::of("", NetAccess::Closed), NetReach::None);
    }
}
