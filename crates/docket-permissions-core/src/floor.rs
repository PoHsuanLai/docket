//! The administrator's floor: what a system file (`/etc/quire/docket-floor.toml`) fixes for the
//! agents on this machine, whatever the person's own settings and files say. It sets how loose
//! the strictness may go and lists locked "always ask" and "never" entries. The person's files
//! cannot loosen either, and nothing here touches the person's own launcher acts.
//!
//! This module holds the shapes and the rules. The daemon that reads the file owns the path and
//! the TOML (`docket-core` never reaches `toml`); a file that exists and does not read is an
//! error there, never a silent skip.

use crate::limit::{Limit, LimitCaller, LimitKey, LimitLevel, LimitMaker, LimitScope};
use crate::mode::{DeleteRow, InsideRow, Mode, ModeRows, OtherRow, ScreenRow, SendRow};
use crate::review::Strictness;
use docket_vocab::ActionRef;
use porter_core::AppName;
use prov::{ActionName, Effect, SpaceScope, UnixSeconds};
use serde::{Deserialize, Serialize};

/// The floor file as written: one optional strictness and a list of locked entries.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloorFile {
    /// The loosest strictness any Space may be set to. Left out: no limit.
    #[serde(default)]
    pub strictness: Option<Strictness>,
    /// Entries no user setting removes.
    #[serde(default)]
    pub locked: Vec<FloorEntry>,
    /// Rows of the modes that always ask, whatever the person's settings say.
    #[serde(default)]
    pub rows: Vec<LockedRow>,
}

/// One row of a mode that the floor holds at "asks".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockedRow {
    /// Changes it can undo, within what the person asked.
    Inside,
    /// Other changes it can undo.
    Other,
    /// Sending.
    Send,
    /// Deleting.
    Delete,
    /// Using another app's window.
    Screen,
}

/// One locked entry as written: `app` and `action` narrow it, `from` is the weakest effect it
/// covers. It covers every Space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FloorEntry {
    /// Ask first, or never.
    pub level: LimitLevel,
    /// The app; left out, every app.
    #[serde(default)]
    pub app: Option<AppName>,
    /// The action; needs `app`.
    #[serde(default)]
    pub action: Option<ActionName>,
    /// The weakest effect it covers; left out, every effect.
    #[serde(default = "every_effect")]
    pub from: Effect,
}

fn every_effect() -> Effect {
    Effect::Read
}

/// Why a floor file does not make sense.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FloorFault {
    /// An entry names an action but not its app.
    #[error("a locked entry names an action without its app")]
    ActionWithoutApp,
}

/// The floor in force. `Floor::none()` is the machine with no floor file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Floor {
    loosest: Option<Strictness>,
    locked: Vec<Limit>,
    rows: Vec<LockedRow>,
}

impl Floor {
    /// No floor.
    pub fn none() -> Self {
        Self::default()
    }

    /// The floor a file describes.
    pub fn from_file(file: FloorFile) -> Result<Self, FloorFault> {
        let locked = file
            .locked
            .into_iter()
            .map(|entry| {
                let scope = match (entry.app, entry.action) {
                    (None, None) => LimitScope::Everything,
                    (Some(app), None) => LimitScope::App(app),
                    (Some(app), Some(name)) => LimitScope::Action(ActionRef { app, name }),
                    (None, Some(_)) => return Err(FloorFault::ActionWithoutApp),
                };
                let key = LimitKey {
                    caller: LimitCaller::AnyAgent,
                    scope,
                    space: SpaceScope::Any,
                    from: entry.from,
                    level: entry.level,
                };
                Ok(Limit::new(key, LimitMaker::Floor, None, UnixSeconds(0)))
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            loosest: file.strictness,
            locked,
            rows: file.rows,
        })
    }

    /// `wanted`, or the loosest the floor allows when that is stricter.
    pub fn clamp(&self, wanted: Strictness) -> Strictness {
        match self.loosest {
            Some(loosest) => wanted.min(loosest),
            None => wanted,
        }
    }

    /// `wanted`, or `Manual` when the floor's loosest strictness is `ask_more`: a floor that asks
    /// for more asking holds every conversation in the mode that asks before every change.
    pub fn clamp_mode(&self, wanted: Mode) -> Mode {
        Mode::from(self.clamp(wanted.strictness()))
    }

    /// Whether the floor holds every conversation in Manual, so none can be switched.
    pub fn holds_manual(&self) -> bool {
        self.clamp_mode(Mode::Auto) == Mode::Manual
    }

    /// `rows` with every row the floor locks set to "asks".
    pub fn clamp_rows(&self, rows: ModeRows) -> ModeRows {
        self.rows.iter().fold(rows, |mut rows, locked| {
            match locked {
                LockedRow::Inside => rows.inside = InsideRow::Asks,
                LockedRow::Other => rows.other = OtherRow::Asks,
                LockedRow::Send => rows.send = SendRow::Asks,
                LockedRow::Delete => rows.delete = DeleteRow::Asks,
                LockedRow::Screen => rows.screen = ScreenRow::Each,
            }
            rows
        })
    }

    /// The rows the floor holds at "asks", for Settings to show as locked.
    pub fn locked_rows(&self) -> &[LockedRow] {
        &self.rows
    }

    /// The locked entries.
    pub fn locked(&self) -> &[Limit] {
        &self.locked
    }
}
