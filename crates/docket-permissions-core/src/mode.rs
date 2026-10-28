//! How a conversation's changes are handled: its mode, and the five rows that say what each
//! mode does without asking. The words are the settings words (`agent.auto.inside`, ...), so the
//! wire, the settings file and the policy files read the same ones.

use crate::review::Strictness;
use serde::{Deserialize, Serialize};

/// How a conversation's changes are handled. Each conversation has its own.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    /// It does what you allowed and asks before sending or deleting.
    #[default]
    #[serde(alias = "default", alias = "trust_more")]
    Auto,
    /// It asks before every change, for when you are watching.
    #[serde(alias = "ask_more")]
    Manual,
}

impl From<Strictness> for Mode {
    fn from(strictness: Strictness) -> Self {
        match strictness {
            Strictness::AskMore => Mode::Manual,
            Strictness::Default | Strictness::TrustMore => Mode::Auto,
        }
    }
}

impl Mode {
    /// What a reviewer is told: Manual reviews with the most care, Auto with the usual.
    pub fn strictness(self) -> Strictness {
        match self {
            Mode::Auto => Strictness::Default,
            Mode::Manual => Strictness::AskMore,
        }
    }

    /// The word the wire, the settings and the policy files use.
    pub fn word(self) -> &'static str {
        match self {
            Mode::Auto => "auto",
            Mode::Manual => "manual",
        }
    }
}

/// Changes it can undo, within what the person asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum InsideRow {
    /// It goes ahead.
    Runs,
    /// It asks.
    Asks,
}

impl InsideRow {
    /// The word the settings and the policy files use.
    pub fn word(self) -> &'static str {
        match self {
            InsideRow::Runs => "runs",
            InsideRow::Asks => "asks",
        }
    }
}

/// Other changes it can undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OtherRow {
    /// Another model looks first, then it goes ahead.
    Check,
    /// It asks.
    Asks,
}

impl OtherRow {
    /// The word the settings and the policy files use.
    pub fn word(self) -> &'static str {
        match self {
            OtherRow::Check => "check",
            OtherRow::Asks => "asks",
        }
    }
}

/// Mail and messages that leave this computer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SendRow {
    /// It asks.
    Asks,
    /// It goes ahead.
    Runs,
}

impl SendRow {
    /// The word the settings and the policy files use.
    pub fn word(self) -> &'static str {
        match self {
            SendRow::Asks => "asks",
            SendRow::Runs => "runs",
        }
    }
}

/// Things that cannot be brought back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeleteRow {
    /// It asks.
    Asks,
    /// It goes ahead.
    Runs,
}

impl DeleteRow {
    /// The word the settings and the policy files use.
    pub fn word(self) -> &'static str {
        match self {
            DeleteRow::Asks => "asks",
            DeleteRow::Runs => "runs",
        }
    }
}

/// Working in another app's window by looking at the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ScreenRow {
    /// It asks once, when the run starts.
    Once,
    /// It asks at each change.
    Each,
}

impl ScreenRow {
    /// The word the settings and the policy files use.
    pub fn word(self) -> &'static str {
        match self {
            ScreenRow::Once => "once",
            ScreenRow::Each => "each",
        }
    }
}

/// What one mode does without asking: the five rows of its section in Settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ModeRows {
    /// Changes it can undo, within what the person asked.
    pub inside: InsideRow,
    /// Other changes it can undo.
    pub other: OtherRow,
    /// Sending.
    pub send: SendRow,
    /// Deleting.
    pub delete: DeleteRow,
    /// Using another app's window.
    pub screen: ScreenRow,
}

impl ModeRows {
    /// The rows Auto starts with.
    pub const AUTO: ModeRows = ModeRows {
        inside: InsideRow::Runs,
        other: OtherRow::Check,
        send: SendRow::Asks,
        delete: DeleteRow::Asks,
        screen: ScreenRow::Once,
    };

    /// The rows Manual starts with.
    pub const MANUAL: ModeRows = ModeRows {
        inside: InsideRow::Asks,
        other: OtherRow::Asks,
        send: SendRow::Asks,
        delete: DeleteRow::Asks,
        screen: ScreenRow::Each,
    };
}

/// The rows of both modes, as the configuration holds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RowSets {
    /// Auto's.
    pub auto: ModeRows,
    /// Manual's.
    pub manual: ModeRows,
}

impl Default for RowSets {
    fn default() -> Self {
        Self {
            auto: ModeRows::AUTO,
            manual: ModeRows::MANUAL,
        }
    }
}

impl RowSets {
    /// The rows of `mode`.
    pub fn of(&self, mode: Mode) -> ModeRows {
        match mode {
            Mode::Auto => self.auto,
            Mode::Manual => self.manual,
        }
    }
}
