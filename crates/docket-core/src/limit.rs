//! Limits: the person's "always ask" and "never" entries, held beside the "allow always" grants.
//! A limit can only tighten: it makes a call that would run ask first, or refuses it without
//! asking. Precedence, strongest first: never, always ask, always allow, the default ruling. An
//! entry with no level (every grant file written before limits) is an always-allow, so a file
//! from before this module reads as it always did.
//!
//! Nothing here reads a clock, a file or the system.

use crate::grant::GrantCaller;
use crate::ids::ActionRef;
use crate::marks::plain_text;
use crate::standing::{Cover, Revocation, StandingGrantId};
use porter_core::AppName;
use prov::{ActionName, Effect, SpaceId, SpaceScope, UnixSeconds};
use serde::{Deserialize, Serialize};

/// What an entry makes of a call, over all three kinds of entry. Ordered by strength, so the
/// strongest of several is their `max`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// The call may skip the question (what a grant without a level always meant).
    #[default]
    AlwaysAllow,
    /// The call always gets a confirmation, even where it would otherwise run.
    AlwaysAsk,
    /// The call is refused without asking.
    Never,
}

impl Level {
    /// The stronger of two levels.
    pub fn over(self, other: Level) -> Level {
        self.max(other)
    }
}

/// The levels a limit can hold: only the two that tighten. The assistant's one action takes
/// this type, so an always-allow cannot be written, and a limit file that says `always_allow`
/// does not read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitLevel {
    /// Always ask first.
    AlwaysAsk,
    /// Never, and say so.
    Never,
}

impl From<LimitLevel> for Level {
    fn from(level: LimitLevel) -> Level {
        match level {
            LimitLevel::AlwaysAsk => Level::AlwaysAsk,
            LimitLevel::Never => Level::Never,
        }
    }
}

impl TryFrom<Level> for LimitLevel {
    type Error = NotATightening;

    fn try_from(level: Level) -> Result<Self, Self::Error> {
        match level {
            Level::AlwaysAllow => Err(NotATightening),
            Level::AlwaysAsk => Ok(LimitLevel::AlwaysAsk),
            Level::Never => Ok(LimitLevel::Never),
        }
    }
}

/// An always-allow is not a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("an always-allow is not a limit")]
pub struct NotATightening;

/// Who a limit binds: every agent, or one caller.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum LimitCaller {
    /// Every agent and every terminal call; never the person's own launcher acts.
    AnyAgent,
    /// This caller alone.
    Only(GrantCaller),
}

/// What a limit covers of the apps.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum LimitScope {
    /// Every action of every app.
    Everything,
    /// Every action of one app.
    App(AppName),
    /// One action.
    Action(ActionRef),
}

/// What identifies a limit: who, what, where, from which effect up, and how hard.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LimitKey {
    /// Who it binds.
    pub caller: LimitCaller,
    /// What it covers.
    pub scope: LimitScope,
    /// Which Spaces.
    pub space: SpaceScope,
    /// It covers calls with this effect or a stronger one (`Read` covers every call).
    pub from: Effect,
    /// How hard it holds.
    pub level: LimitLevel,
}

/// A call, as a limit sees it.
#[derive(Debug, Clone, Copy)]
pub struct LimitCall<'a> {
    /// Who is calling.
    pub caller: &'a GrantCaller,
    /// The app that owns the action.
    pub app: &'a AppName,
    /// The action; none for a question about the whole app (a computer-use grant).
    pub action: Option<&'a ActionName>,
    /// The Space of the call.
    pub space: &'a SpaceId,
    /// The call's effect.
    pub effect: Effect,
}

impl LimitKey {
    /// Whether the key covers `call`.
    pub fn covers(&self, call: &LimitCall<'_>) -> Cover {
        let who = match &self.caller {
            LimitCaller::AnyAgent => true,
            LimitCaller::Only(caller) => caller == call.caller,
        };
        let what = match &self.scope {
            LimitScope::Everything => true,
            LimitScope::App(app) => app == call.app,
            LimitScope::Action(action) => {
                action.app == *call.app && call.action == Some(&action.name)
            }
        };
        let place = match &self.space {
            SpaceScope::Any => true,
            SpaceScope::Only(space) => space == call.space,
        };
        if who && what && place && call.effect >= self.from {
            Cover::Covers
        } else {
            Cover::Misses
        }
    }
}

/// The person's own words, kept to show why a limit exists: 1 to 160 characters, one line.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Said(String);

/// Why text is not a saying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("words kept with a limit are 1 to 160 characters on one line")]
pub struct SaidFault;

impl Said {
    /// `text` as a saying.
    pub fn parse(text: &str) -> Result<Self, SaidFault> {
        let text = text.trim();
        // The words are drawn on a sheet and in the activity view: the same marks that a
        // reviewer's reason may not hold (controls, bidirectional overrides, hidden marks) are
        // refused here too.
        if text.is_empty() || text.chars().count() > 160 || !plain_text(text, &[]) {
            Err(SaidFault)
        } else {
            Ok(Self(text.to_owned()))
        }
    }

    /// The words.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Said {
    type Error = SaidFault;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<Said> for String {
    fn from(said: Said) -> String {
        said.0
    }
}

/// Who made a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitMaker {
    /// The person: from a sheet's "never", or in Settings.
    Person,
    /// The assistant, tightening on the person's word.
    Assistant,
    /// The administrator's floor.
    Floor,
}

/// One limit, as the store keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Limit {
    /// Its identity: the same as a grant's, so the one revoke path removes it.
    pub id: StandingGrantId,
    /// What it is.
    pub key: LimitKey,
    /// Who made it.
    pub by: LimitMaker,
    /// The person's words, when the assistant made it from a sentence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub said: Option<Said>,
    /// When.
    pub at: UnixSeconds,
}

/// Whether the sheet may offer Undo for a limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoOffer {
    /// The person made it or can unmake it: the revoke path removes it.
    Offered,
    /// The administrator's: no revoke removes it.
    Withheld,
}

/// Why a call asks or is refused, in plain words, with the id Undo removes.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LimitNote {
    /// The entry.
    pub id: StandingGrantId,
    /// How hard it holds.
    pub level: LimitLevel,
    /// The short reason, in plain words.
    pub why: String,
    /// Whether Undo is offered.
    pub undo: UndoOffer,
}

impl Limit {
    /// The limit with `key`, made by `by` at `at`.
    pub fn new(key: LimitKey, by: LimitMaker, said: Option<Said>, at: UnixSeconds) -> Self {
        Self {
            id: StandingGrantId::of_limit(&key),
            key,
            by,
            said,
            at,
        }
    }

    /// The reason a call meets this limit: the person's words when they are kept, else a plain
    /// phrase made from the level and the maker.
    pub fn note(&self) -> LimitNote {
        let why = match (&self.said, self.by, self.key.level) {
            (Some(said), _, _) => format!("You said: \u{201c}{}\u{201d}", said.as_str()),
            (None, LimitMaker::Floor, LimitLevel::AlwaysAsk) => {
                "Your administrator requires asking first.".to_owned()
            }
            (None, LimitMaker::Floor, LimitLevel::Never) => {
                "Your administrator does not allow this.".to_owned()
            }
            (None, _, LimitLevel::AlwaysAsk) => "You asked to be asked first.".to_owned(),
            (None, _, LimitLevel::Never) => "You said never to do this.".to_owned(),
        };
        let undo = match self.by {
            LimitMaker::Floor => UndoOffer::Withheld,
            LimitMaker::Person | LimitMaker::Assistant => UndoOffer::Offered,
        };
        LimitNote {
            id: self.id.clone(),
            level: self.key.level,
            why,
            undo,
        }
    }
}

/// What the limits say about one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Limited {
    /// No limit covers it: the grants and the default ruling decide.
    Free,
    /// It always asks, for this reason.
    Ask(LimitNote),
    /// It is refused without asking, for this reason.
    Never(LimitNote),
}

/// What `held` (the person's and the assistant's limits) and `locked` (the administrator's) say
/// about `call`: the strongest covering limit wins, and the administrator's wins a tie, so its
/// reason is the one shown. The person's own grants never enter: no always-allow outranks a limit.
pub fn limited(held: &[Limit], locked: &[Limit], call: &LimitCall<'_>) -> Limited {
    // `max_by_key` keeps the last of equal keys, so the locked entries go last.
    held.iter()
        .chain(locked)
        .filter(|l| l.key.covers(call) == Cover::Covers)
        .max_by_key(|l| l.key.level)
        .map_or(Limited::Free, |l| match l.key.level {
            LimitLevel::AlwaysAsk => Limited::Ask(l.note()),
            LimitLevel::Never => Limited::Never(l.note()),
        })
}

impl From<&Limited> for Level {
    /// The level a limit's verdict stands for: no limit is the lowest.
    fn from(limited: &Limited) -> Level {
        match limited {
            Limited::Free => Level::AlwaysAllow,
            Limited::Ask(_) => Level::AlwaysAsk,
            Limited::Never(_) => Level::Never,
        }
    }
}

/// What the assistant may ask for: a tightening of its own. The level type holds no allow, so
/// nothing built from this can loosen anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tighten {
    /// Ask first, or never.
    pub level: LimitLevel,
    /// What it covers.
    pub scope: LimitScope,
    /// Which Spaces.
    pub space: SpaceScope,
    /// From which effect up.
    pub from: Effect,
    /// The person's words, when they were kept.
    pub said: Option<Said>,
}

impl Tighten {
    /// The limit this makes, at `at`. It binds every agent.
    pub fn limit(self, at: UnixSeconds) -> Limit {
        let key = LimitKey {
            caller: LimitCaller::AnyAgent,
            scope: self.scope,
            space: self.space,
            from: self.from,
            level: self.level,
        };
        Limit::new(key, LimitMaker::Assistant, self.said, at)
    }
}

/// `limits` with `limit` held: the same id replaces the earlier one.
pub fn limits_with(mut limits: Vec<Limit>, limit: Limit) -> Vec<Limit> {
    limits.retain(|l| l.id != limit.id);
    limits.push(limit);
    limits
}

/// `limits` without the limit `id`, and whether there was one.
pub fn limits_without(mut limits: Vec<Limit>, id: &StandingGrantId) -> (Vec<Limit>, Revocation) {
    let before = limits.len();
    limits.retain(|l| l.id != *id);
    let done = if limits.len() < before {
        Revocation::Revoked
    } else {
        Revocation::NotHeld
    };
    (limits, done)
}

/// The limits a file's text holds: an empty text is none.
pub fn decode_limits(text: &str) -> Result<Vec<Limit>, serde_json::Error> {
    match text.trim() {
        "" => Ok(Vec::new()),
        json => serde_json::from_str(json),
    }
}

/// The text of a list of limits. An error is never turned into an empty list.
pub fn encode_limits(limits: &[Limit]) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(limits)
}
