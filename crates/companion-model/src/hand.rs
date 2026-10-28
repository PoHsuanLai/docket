//! A conversation with an agent app moving between docket and the app's own command-line
//! program in a terminal. The session at the router stays one session: it is a watch session
//! (restore points only) while the person works in the terminal, and an agent session again when
//! the conversation is brought back. These are the words for it that the router, the host of the
//! agent apps and the terminal share.

use docket_permissions_core::ProgramName;
use docket_vocab::Workspace;
use prov::{AgentLabel, SessionId};
use serde::{Deserialize, Serialize};
use std::fmt;

/// The longest app conversation id kept, in bytes.
const APP_ID_MAX: usize = 128;
/// The most words of a seen command line kept.
const SEEN_TOKENS_MAX: usize = 256;
/// The most bytes of a seen command line kept, all words together.
const SEEN_BYTES_MAX: usize = 64 * 1024;

/// An agent app's own id for one conversation. It goes on a command line and the app says it, so
/// it must be one plain word (letters, digits, `-`, `_`, `.`, `:`; at most 128 bytes) that no
/// program takes for an option (no leading `-`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AppSessionId(String);

/// Why text is not an app conversation id.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("an app conversation id is one plain word of at most 128 characters, not starting with -")]
pub struct AppSessionIdError;

impl AppSessionId {
    /// `text` as an app conversation id.
    pub fn parse(text: &str) -> Result<Self, AppSessionIdError> {
        let word = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':');
        let plain = !text.is_empty()
            && text.len() <= APP_ID_MAX
            && !text.starts_with('-')
            && text.chars().all(word);
        if plain {
            Ok(Self(text.to_owned()))
        } else {
            Err(AppSessionIdError)
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for AppSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for AppSessionId {
    type Error = AppSessionIdError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<AppSessionId> for String {
    fn from(id: AppSessionId) -> String {
        id.0
    }
}

/// The command line a terminal saw running in one of its panes, word by word. It comes from
/// outside and can hold secrets: it is only matched, never stored or logged, and its `Debug`
/// shows how many words it has and nothing else.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<String>", into = "Vec<String>")]
pub struct SeenCommand(Vec<String>);

/// Why words are not a seen command line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SeenCommandFault {
    /// There are no words.
    #[error("the command line is empty")]
    Empty,
    /// There are more than 256 words.
    #[error("the command line has too many words")]
    TooManyWords,
    /// The words together are longer than 64 KiB.
    #[error("the command line is too long")]
    TooLong,
    /// A word holds a NUL byte, which no command line can.
    #[error("the command line holds a NUL byte")]
    Nul,
}

impl SeenCommand {
    /// `words` as a seen command line.
    pub fn parse(words: Vec<String>) -> Result<Self, SeenCommandFault> {
        if words.is_empty() {
            return Err(SeenCommandFault::Empty);
        }
        if words.len() > SEEN_TOKENS_MAX {
            return Err(SeenCommandFault::TooManyWords);
        }
        if words.iter().map(String::len).sum::<usize>() > SEEN_BYTES_MAX {
            return Err(SeenCommandFault::TooLong);
        }
        if words.iter().any(|w| w.contains('\0')) {
            return Err(SeenCommandFault::Nul);
        }
        Ok(Self(words))
    }

    /// The words, the program first.
    pub fn tokens(&self) -> &[String] {
        &self.0
    }
}

impl fmt::Debug for SeenCommand {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SeenCommand({} words)", self.0.len())
    }
}

impl TryFrom<Vec<String>> for SeenCommand {
    type Error = SeenCommandFault;

    fn try_from(words: Vec<String>) -> Result<Self, Self::Error> {
        Self::parse(words)
    }
}

impl From<SeenCommand> for Vec<String> {
    fn from(seen: SeenCommand) -> Vec<String> {
        seen.0
    }
}

/// An agent app and its own id for one conversation, which is what the app's command-line
/// program picks up again. Said by the host that ran the app or by the terminal that watches it;
/// never read from a model's words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchedAgent {
    /// The configured program (`agents.toml`).
    pub program: ProgramName,
    /// The app's own id for the conversation, as the app prints it.
    pub session: AppSessionId,
}

/// Who closes a watch session, and so who may be told it is there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchOwner {
    /// The terminal opened the session and closes it.
    Terminal,
    /// The session is the companion's, handed over to the terminal: the terminal marks turns on
    /// it and never closes it.
    Companion,
}

/// The answer to a terminal that names the agent it watches: the watch session to mark turns on,
/// and who closes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Watching {
    /// The watch session.
    pub session: SessionId,
    /// Who closes it.
    pub owner: WatchOwner,
}

/// Where a conversation goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HandTo {
    /// The app's own program in a terminal: the session becomes a watch session.
    Terminal,
    /// Back to docket: the session is an agent session again.
    Desktop,
}

/// What the log of a session says about where its conversation is, and with whom.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandNote {
    /// Where the conversation went.
    pub to: HandTo,
    /// The app and its id for the conversation, when they are known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<WatchedAgent>,
}

/// One watch session whose agent a terminal named: what the shell can offer to bring back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchedLine {
    /// The watch session.
    pub session: SessionId,
    /// The app and its own id for the conversation.
    pub agent: WatchedAgent,
    /// What the person calls the agent there, as its opener said (display text).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<AgentLabel>,
    /// The folder the agent works in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<Workspace>,
}
