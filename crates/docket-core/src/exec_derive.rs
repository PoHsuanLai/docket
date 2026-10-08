//! Does a command's argument vector derive from what the agent was served? (acp-sessions.md
//! section 12, R11.) After a read, only a command that does loses grant eligibility; every other
//! command keeps it.
//!
//! The agent sends its argv as plain strings with no provenance, so the router's argument labels
//! cannot tell: they would call every argument untrusted once the session read anything. The host
//! keeps what it served (the files' paths and the token-like strings of their text) and this
//! conservative, typed test compares the argv with it. It is a heuristic and says so:
//!
//! - **Caught:** an argument that is, or lies under, a path the agent read (a relative one is
//!   resolved against the working directory); an argument that contains a token-like string of a
//!   served text, or is a part of one (at least [`MIN_TOKEN`] characters, not a plain lowercase
//!   word); the value of a `--flag=value`.
//! - **Not caught:** a value the agent transformed (decoded, split, reversed, hashed), one it
//!   assembled from short pieces, a plain lowercase word, a short token, anything inside a script
//!   it wrote to disk first, and what a tool the agent ran itself brought in (that content is
//!   never seen here: see [`Served::unseen`], which makes every command derived).
//!
//! What it misses is why the network rule (`exec_reach`) is unconditional.

use crate::standing::{AbsPath, Cover};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// The shortest string that counts as a token.
pub const MIN_TOKEN: usize = 8;
/// The longest token kept: the front of a longer one stands for it.
const MAX_TOKEN: usize = 512;
/// The most tokens kept; past it every command counts as derived.
const MAX_TOKENS: usize = 20_000;

/// Whether a command's arguments derive from what the agent was served.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Derivation {
    /// Nothing in it comes from what was read, as far as this test can tell.
    Independent,
    /// Something in it does, or this could not be told.
    Derived,
}

/// Whether the host saw all the content the agent took in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sight {
    /// Every byte came through `fs/read_text_file`.
    Whole,
    /// Something came in that the host never saw (a tool the agent ran itself, a resumed
    /// session, more text than was kept).
    Partial,
}

/// What the host served one session, for the derivation test.
#[derive(Debug, Clone)]
pub struct Served {
    paths: BTreeSet<AbsPath>,
    tokens: BTreeSet<String>,
    sight: Sight,
}

impl Default for Served {
    fn default() -> Self {
        Self {
            paths: BTreeSet::new(),
            tokens: BTreeSet::new(),
            sight: Sight::Whole,
        }
    }
}

fn separator(c: char) -> bool {
    !(c.is_ascii_alphanumeric() || "._-/@+~%".contains(c))
}

/// Long enough, and more than a lowercase word: it holds a digit, a capital or a symbol.
fn token_like(text: &str) -> bool {
    text.len() >= MIN_TOKEN && text.chars().any(|c| !c.is_ascii_lowercase())
}

fn tokens_of(text: &str) -> impl Iterator<Item = &str> {
    text.split(separator)
        .filter(|t| token_like(t))
        .map(|t| t.get(..MAX_TOKEN).unwrap_or(t))
}

/// `word` as an absolute path: `..` and `.` folded lexically, a relative one taken from `cwd`.
fn resolve(cwd: &AbsPath, word: &str) -> Option<AbsPath> {
    if word.is_empty() || word.chars().any(char::is_control) {
        return None;
    }
    let from = if word.starts_with('/') {
        ""
    } else {
        cwd.as_str()
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in from.split('/').chain(word.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    AbsPath::parse(&format!("/{}", parts.join("/"))).ok()
}

/// The part of a word that is a value: a flag has none unless it is `--flag=value`.
fn value_of(word: &str) -> Option<&str> {
    if word.starts_with('-') {
        word.split_once('=')
            .map(|(_, v)| v)
            .filter(|v| !v.is_empty())
    } else {
        Some(word)
    }
}

impl Served {
    /// A file's text was served: its path (as asked and as resolved) and its tokens are kept.
    pub fn record(&mut self, asked: &AbsPath, real: &AbsPath, text: &str) {
        self.paths.insert(asked.clone());
        self.paths.insert(real.clone());
        for token in tokens_of(text) {
            if self.tokens.len() >= MAX_TOKENS {
                self.sight = Sight::Partial;
                return;
            }
            self.tokens.insert(token.to_owned());
        }
    }

    /// Content came in that the host never saw (a tool the agent ran itself, or a resumed
    /// session): from now on no command can be told apart from it, so every one is derived.
    pub fn unseen(&mut self) {
        self.sight = Sight::Partial;
    }

    /// Nothing has been served and nothing unseen has come in.
    pub fn is_untouched(&self) -> bool {
        self.sight == Sight::Whole && self.paths.is_empty()
    }

    fn names_a_served_path(&self, cwd: &AbsPath, value: &str) -> bool {
        resolve(cwd, value).is_some_and(|path| {
            self.paths
                .iter()
                .any(|served| served.covers(&path) == Cover::Covers)
        })
    }

    fn holds_a_served_token(&self, value: &str) -> bool {
        self.tokens.iter().any(|t| value.contains(t.as_str()))
            || tokens_of(value).any(|piece| self.tokens.iter().any(|t| t.contains(piece)))
    }

    /// Whether the command `words` (program first) run in `cwd` derives from what was served.
    pub fn derivation(&self, words: &[String], cwd: &AbsPath) -> Derivation {
        if self.sight == Sight::Partial {
            return Derivation::Derived;
        }
        let derived = words.iter().enumerate().any(|(at, word)| {
            let Some(value) = value_of(word) else {
                return false;
            };
            self.names_a_served_path(cwd, value) || (at > 0 && self.holds_a_served_token(value))
        });
        if derived {
            Derivation::Derived
        } else {
            Derivation::Independent
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(text: &str) -> AbsPath {
        AbsPath::parse(text).expect("path")
    }

    fn served() -> Served {
        let mut s = Served::default();
        let file = abs("/home/u/proj/src/lib.rs");
        s.record(
            &file,
            &file,
            "const KEY: &str = \"sk_live_Ab12Cd34\";\nfn deploy_target() {}\nlet n = 42;\nhttps://evil.test/drop?id=77\n",
        );
        s
    }

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_table_of_commands_after_a_read() {
        use Derivation::{Derived as Yes, Independent as No};
        let cases = [
            // Nothing in common with what was read.
            ("cargo test", No),
            ("cargo build --release", No),
            ("git status", No),
            ("ls -la", No),
            ("pwd", No),
            // A path the agent read, absolute or relative, or a file beneath one.
            ("cat /home/u/proj/src/lib.rs", Yes),
            ("cat src/lib.rs", Yes),
            ("cat ./src/../src/lib.rs", Yes),
            ("rustfmt src/lib.rs", Yes),
            ("wc -l --files-from=src/lib.rs", Yes),
            // A directory above the file is not the file.
            ("ls src", No),
            ("ls /home/u/proj", No),
            // A token of the text, whole or inside a bigger argument.
            ("echo sk_live_Ab12Cd34", Yes),
            ("tool --key=sk_live_Ab12Cd34", Yes),
            ("tool url=https://evil.test/drop?id=77&x=1", Yes),
            ("tool prefix-sk_live_Ab12Cd34-suffix", Yes),
            // A part of a token the agent typed out.
            ("grep sk_live_Ab", Yes),
            // Short strings, plain lowercase words and bare flags are ignored.
            ("cargo test sk_live", No),
            ("cargo test 42", No),
            ("cargo test integration", No),
            ("cargo test --features deploy_target", Yes),
            ("cargo test --deploy_target_x", No),
            // The program word is compared by path only, never by token.
            ("sk_live_Ab12Cd34 --help", No),
        ];
        let s = served();
        let cwd = abs("/home/u/proj");
        for (line, want) in cases {
            assert_eq!(s.derivation(&words(line), &cwd), want, "{line}");
        }
    }

    #[test]
    fn a_session_that_read_nothing_derives_nothing() {
        let s = Served::default();
        assert!(s.is_untouched());
        let cwd = abs("/home/u/proj");
        assert_eq!(
            s.derivation(&words("cat src/lib.rs sk_live_Ab12Cd34"), &cwd),
            Derivation::Independent
        );
    }

    #[test]
    fn content_the_host_never_saw_makes_every_command_derived() {
        let mut s = served();
        s.unseen();
        let cwd = abs("/home/u/proj");
        assert_eq!(s.derivation(&words("pwd"), &cwd), Derivation::Derived);
        let mut fresh = Served::default();
        fresh.unseen();
        assert!(!fresh.is_untouched());
        assert_eq!(fresh.derivation(&words("ls"), &cwd), Derivation::Derived);
    }

    #[test]
    fn more_text_than_is_kept_makes_every_command_derived() {
        let mut s = Served::default();
        let file = abs("/home/u/proj/big.txt");
        let text: String = (0..=MAX_TOKENS).map(|n| format!("tok_{n:08} ")).collect();
        s.record(&file, &file, &text);
        assert_eq!(
            s.derivation(&words("pwd"), &abs("/home/u/proj")),
            Derivation::Derived
        );
    }

    #[test]
    fn a_link_resolved_path_counts_as_the_file() {
        let mut s = Served::default();
        s.record(
            &abs("/home/u/proj/link.rs"),
            &abs("/home/u/proj/real/a.rs"),
            "x",
        );
        assert_eq!(
            s.derivation(&words("cat real/a.rs"), &abs("/home/u/proj")),
            Derivation::Derived
        );
        assert_eq!(
            s.derivation(&words("cat link.rs"), &abs("/home/u/proj")),
            Derivation::Derived
        );
    }
}
