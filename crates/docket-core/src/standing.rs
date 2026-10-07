//! Standing grants: "allow always" as a scoped, revocable grant held in docket's own store
//! (acp-sessions.md section 12, R1). A grant replaces only the confirmation step; the whole gate
//! still runs on every call. The types here are the key (who, which action, which arguments);
//! `standing_match` decides whether a call is covered and `standing_offer` whether "always" may
//! be offered at all. Nothing here reads a clock, a file or the system.

use crate::grant::GrantCaller;
use crate::ids::ActionRef;
use prov::UnixSeconds;
use serde::{Deserialize, Serialize};

/// Whether a scope covers a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cover {
    /// Every argument the scope names lies inside it.
    Covers,
    /// Something lies outside it, or cannot be read.
    Misses,
}

/// A directory (or file) as an absolute, normalised path: no `.` or `..` steps, no empty
/// components, no trailing slash. Compared by whole components, so `/a/b` does not cover `/a/bc`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AbsPath(String);

/// Why text is not an absolute path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PathFault {
    /// It does not start with `/`.
    #[error("a path must be absolute")]
    NotAbsolute,
    /// It steps up with `..`, which could leave the scope.
    #[error("a path must not step up with ..")]
    ParentStep,
    /// It holds a control character.
    #[error("a path must not hold control characters")]
    Control,
}

impl AbsPath {
    /// `text` as an absolute path, `.` and doubled slashes folded away.
    pub fn parse(text: &str) -> Result<Self, PathFault> {
        if !text.starts_with('/') {
            return Err(PathFault::NotAbsolute);
        }
        if text.chars().any(char::is_control) {
            return Err(PathFault::Control);
        }
        let mut parts = Vec::new();
        for part in text.split('/') {
            match part {
                "" | "." => {}
                ".." => return Err(PathFault::ParentStep),
                other => parts.push(other),
            }
        }
        Ok(Self(format!("/{}", parts.join("/"))))
    }

    /// The path as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Whether this is `/`, which no grant may use as its prefix.
    pub fn is_root(&self) -> RootState {
        if self.0 == "/" {
            RootState::Root
        } else {
            RootState::Below
        }
    }

    /// Whether `other` is this path or lies below it, by whole components.
    pub fn covers(&self, other: &AbsPath) -> Cover {
        let under = match self.is_root() {
            RootState::Root => true,
            RootState::Below => other
                .0
                .strip_prefix(&self.0)
                .is_some_and(|rest| rest.is_empty() || rest.starts_with('/')),
        };
        if under { Cover::Covers } else { Cover::Misses }
    }

    /// The directory above, none for `/`.
    pub fn parent(&self) -> Option<AbsPath> {
        let cut = self.0.rfind('/')?;
        match self.is_root() {
            RootState::Root => None,
            RootState::Below => Some(AbsPath(if cut == 0 {
                "/".to_owned()
            } else {
                self.0[..cut].to_owned()
            })),
        }
    }

    /// The deepest path that covers both.
    pub fn common(&self, other: &AbsPath) -> AbsPath {
        let mut here = Some(self.clone());
        while let Some(path) = here {
            if path.covers(other) == Cover::Covers {
                return path;
            }
            here = path.parent();
        }
        AbsPath("/".to_owned())
    }
}

/// Whether a path is `/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RootState {
    /// It is `/`.
    Root,
    /// It is below `/`.
    Below,
}

impl TryFrom<String> for AbsPath {
    type Error = PathFault;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<AbsPath> for String {
    fn from(path: AbsPath) -> String {
        path.0
    }
}

/// A host name, lower case: `example.org`. Matched whole, never as a suffix, so `example.org`
/// does not cover `evilexample.org`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Domain(String);

/// Why text is not a host name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a domain is 1 to 253 letters, digits, dots and hyphens")]
pub struct DomainFault;

impl Domain {
    /// `text` as a domain.
    pub fn parse(text: &str) -> Result<Self, DomainFault> {
        let lower = text.trim_end_matches('.').to_ascii_lowercase();
        let ok = !lower.is_empty()
            && lower.len() <= 253
            && !lower.starts_with(['.', '-'])
            && !lower.contains("..")
            && lower
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-');
        if ok {
            Ok(Self(lower))
        } else {
            Err(DomainFault)
        }
    }

    /// The domain as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Domain {
    type Error = DomainFault;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<Domain> for String {
    fn from(domain: Domain) -> String {
        domain.0
    }
}

/// Where an outbound call goes: one address, or every address at one domain.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Recipient {
    /// `name@example.org`, lower case, exactly one `@`.
    Address(String),
    /// Everything at this domain (a grant), or a host a URL names (a call).
    Domain(Domain),
}

/// Why text is not an address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an address is a name, one @ and a domain")]
pub struct AddressFault;

impl Recipient {
    /// `text` as an address.
    pub fn address(text: &str) -> Result<Self, AddressFault> {
        let lower = text.trim().to_ascii_lowercase();
        match lower.split_once('@') {
            Some((name, host))
                if !name.is_empty()
                    && !name.chars().any(|c| c.is_control() || c.is_whitespace())
                    && !host.contains('@')
                    && Domain::parse(host).is_ok() =>
            {
                Ok(Self::Address(lower))
            }
            _ => Err(AddressFault),
        }
    }

    /// The domain part of an address, or the domain itself.
    fn host(&self) -> Option<Domain> {
        match self {
            Recipient::Address(a) => a.split_once('@').and_then(|(_, h)| Domain::parse(h).ok()),
            Recipient::Domain(d) => Some(d.clone()),
        }
    }

    /// Whether this grant recipient covers a recipient a call names. An address grant covers
    /// that address alone; a domain grant covers addresses at the domain, and a URL's host.
    pub fn covers(&self, call: &Recipient) -> Cover {
        let same = match self {
            Recipient::Address(_) => self == call,
            Recipient::Domain(d) => call.host().as_ref() == Some(d),
        };
        if same { Cover::Covers } else { Cover::Misses }
    }
}

/// The leading words a terminal command must start with: `cargo test`. Words only: none holds a
/// shell operator, quote or expansion, so a prefix never grants a pipeline.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct CommandPrefix(Vec<String>);

/// Why text is not a command prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CommandFault {
    /// No words.
    #[error("a command prefix needs at least one word")]
    Empty,
    /// A shell operator, quote, expansion or control character.
    #[error("a command prefix is plain words")]
    Shell,
}

/// Characters that let a shell run something other than the words typed.
const SHELL: &[char] = &[
    ';', '&', '|', '<', '>', '(', ')', '`', '$', '\\', '"', '\'', '*', '?', '{', '}', '!',
];

fn plain(text: &str) -> bool {
    !text.chars().any(|c| c.is_control() || SHELL.contains(&c))
}

impl CommandPrefix {
    /// `text` as a prefix.
    pub fn parse(text: &str) -> Result<Self, CommandFault> {
        if !plain(text) {
            return Err(CommandFault::Shell);
        }
        let words: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
        if words.is_empty() {
            Err(CommandFault::Empty)
        } else {
            Ok(Self(words))
        }
    }

    /// The prefix as text.
    pub fn as_text(&self) -> String {
        self.0.join(" ")
    }

    /// Whether `command` starts with these words and is otherwise plain: a command with an
    /// operator, expansion or newline anywhere is never covered, whatever it starts with.
    pub fn covers(&self, command: &str) -> Cover {
        let words: Vec<&str> = command.split_whitespace().collect();
        let starts = words.len() >= self.0.len() && self.0.iter().zip(&words).all(|(a, b)| a == b);
        if starts && plain(command) {
            Cover::Covers
        } else {
            Cover::Misses
        }
    }
}

impl TryFrom<String> for CommandPrefix {
    type Error = CommandFault;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<CommandPrefix> for String {
    fn from(prefix: CommandPrefix) -> String {
        prefix.as_text()
    }
}

/// What a standing grant covers: an action, narrowed by one typed argument scope.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum StandingScope {
    /// Files: this action on paths at or below a directory.
    Files {
        /// The action.
        action: ActionRef,
        /// The directory; never `/`.
        under: AbsPath,
    },
    /// Terminal: this action for commands starting with a prefix, run in or below a directory.
    Terminal {
        /// The action.
        action: ActionRef,
        /// The leading words.
        command: CommandPrefix,
        /// The working directory; never `/`.
        cwd: AbsPath,
    },
    /// Outbound: this action to one recipient or domain.
    Outbound {
        /// The action.
        action: ActionRef,
        /// Where it goes.
        to: Recipient,
    },
}

/// Which of the three scopes, without its arguments (what the audit keeps).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeKind {
    /// [`StandingScope::Files`].
    Files,
    /// [`StandingScope::Terminal`].
    Terminal,
    /// [`StandingScope::Outbound`].
    Outbound,
}

impl StandingScope {
    /// The action it covers.
    pub fn action(&self) -> &ActionRef {
        match self {
            StandingScope::Files { action, .. }
            | StandingScope::Terminal { action, .. }
            | StandingScope::Outbound { action, .. } => action,
        }
    }

    /// Its kind.
    pub fn kind(&self) -> ScopeKind {
        match self {
            StandingScope::Files { .. } => ScopeKind::Files,
            StandingScope::Terminal { .. } => ScopeKind::Terminal,
            StandingScope::Outbound { .. } => ScopeKind::Outbound,
        }
    }

    /// Whether the scope is narrow enough to hold: a directory scope is never `/`.
    pub fn narrow(&self) -> NarrowState {
        let rooted = match self {
            StandingScope::Files { under, .. } => under.is_root(),
            StandingScope::Terminal { cwd, .. } => cwd.is_root(),
            StandingScope::Outbound { .. } => RootState::Below,
        };
        match rooted {
            RootState::Root => NarrowState::TooBroad,
            RootState::Below => NarrowState::Narrow,
        }
    }
}

/// Whether a scope is narrow enough to hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NarrowState {
    /// Yes.
    Narrow,
    /// It covers the whole file system.
    TooBroad,
}

/// A grant's identity: derived from its caller and scope, so granting the same thing twice is one
/// grant and a restart never reuses an id for a different grant. `sg-` and 16 hex digits.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct StandingGrantId(String);

/// Why text is not a standing grant id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a standing grant id is `sg-` and 16 hex digits")]
pub struct StandingIdFault;

impl StandingGrantId {
    /// The id of the grant for `caller` and `scope`.
    pub fn of(caller: &GrantCaller, scope: &StandingScope) -> Self {
        let bytes = serde_json::to_vec(&(caller, scope)).unwrap_or_default();
        let hash = bytes.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
            (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
        });
        Self(format!("sg-{hash:016x}"))
    }

    /// `text` as an id.
    pub fn parse(text: &str) -> Result<Self, StandingIdFault> {
        match text.strip_prefix("sg-") {
            Some(hex)
                if hex.len() == 16 && hex.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')) =>
            {
                Ok(Self(text.to_owned()))
            }
            _ => Err(StandingIdFault),
        }
    }

    /// The id as text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for StandingGrantId {
    type Error = StandingIdFault;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<StandingGrantId> for String {
    fn from(id: StandingGrantId) -> String {
        id.0
    }
}

/// One standing grant, as the store keeps it and Settings lists it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StandingGrant {
    /// Its identity.
    pub id: StandingGrantId,
    /// The one caller it belongs to; another caller is not covered.
    pub caller: GrantCaller,
    /// What it covers.
    pub scope: StandingScope,
    /// When the person gave it.
    pub at: UnixSeconds,
}

impl StandingGrant {
    /// The grant `caller` holds for `scope`, given at `at`.
    pub fn new(caller: GrantCaller, scope: StandingScope, at: UnixSeconds) -> Self {
        Self {
            id: StandingGrantId::of(&caller, &scope),
            caller,
            scope,
            at,
        }
    }
}

/// What a revocation did.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Revocation {
    /// The grant is gone; the next call asks again.
    Revoked,
    /// There was no such grant.
    NotHeld,
}

/// `grants` with `grant` held: the same id replaces the earlier one.
pub fn held_with(mut grants: Vec<StandingGrant>, grant: StandingGrant) -> Vec<StandingGrant> {
    grants.retain(|g| g.id != grant.id);
    grants.push(grant);
    grants
}

/// `grants` without the grant `id`, and whether there was one.
pub fn held_without(
    mut grants: Vec<StandingGrant>,
    id: &StandingGrantId,
) -> (Vec<StandingGrant>, Revocation) {
    let before = grants.len();
    grants.retain(|g| g.id != *id);
    let done = if grants.len() < before {
        Revocation::Revoked
    } else {
        Revocation::NotHeld
    };
    (grants, done)
}

/// The grants a file's text holds: an empty text is none.
pub fn decode_standing(text: &str) -> Result<Vec<StandingGrant>, String> {
    match text.trim() {
        "" => Ok(Vec::new()),
        json => serde_json::from_str(json).map_err(|why| why.to_string()),
    }
}

/// The text of a list of grants.
pub fn encode_standing(grants: &[StandingGrant]) -> String {
    serde_json::to_string_pretty(grants).unwrap_or_else(|_| "[]".to_owned())
}
