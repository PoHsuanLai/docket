//! What an agent last offered, written by docket whenever a session with it opens (and when its
//! models are listed), kept at `state/<id>/offered.toml` for the Settings app to read.
//!
//! ```toml
//! seen = 3                    # how many times this was written: a counter, no clock
//! start = "signed_in"         # signed_in | needs_sign_in | unknown
//! in_use = "sonnet"           # the model the session ran, when the agent says
//! [[model]]
//! id = "sonnet"
//! name = "Sonnet 5.5"
//! [[way]]                     # the ways of signing in the agent listed
//! id = "login"
//! name = "Sign in with your account"
//! ```
//!
//! The record is the agent's own words, shown to the person and trusted for nothing: the ids the
//! person picks are checked again by the agent when the next session opens.

use crate::dirs::AgentsDir;
use crate::slug::Slug;
use docket_core::{read_optional, write_atomic};
use serde::{Deserialize, Serialize};

/// The file name of the record, inside the agent's state directory.
pub const FILE: &str = "offered.toml";

/// A thing the agent lists: its id as `agents.toml` writes it, and the name a person reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Named {
    /// The id.
    pub id: String,
    /// The agent's name for it.
    pub name: String,
}

/// How the last start went, as far as sign-in goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Start {
    /// The session opened.
    SignedIn,
    /// The agent asked for a sign-in first; the ways it listed are in `ways`.
    NeedsSignIn,
    /// The start did not get as far as sign-in.
    Unknown,
}

/// What the agent offered at its last start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Offered {
    /// How many times the record was written (a counter; the record never reads a clock).
    pub seen: u64,
    /// How the last start went.
    pub start: Start,
    /// The model the session ran, when the agent says.
    #[serde(default)]
    pub in_use: Option<String>,
    /// The models on offer; none when the agent offers no choice.
    #[serde(default, rename = "model")]
    pub models: Vec<Named>,
    /// The ways of signing in the agent listed.
    #[serde(default, rename = "way")]
    pub ways: Vec<Named>,
}

/// Why a record was not read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum OfferedFault {
    /// The id is not a name docket keeps agents under.
    #[error("the agent's name is not one docket keeps records under")]
    Name,
    /// The file could not be read or written.
    #[error("the record could not be read or written")]
    Io,
    /// The file is not a record.
    #[error("the record is damaged")]
    Damaged,
}

impl Offered {
    /// Reads the record of `id`; none when the agent never started.
    pub fn read(dir: &AgentsDir, id: &str) -> Result<Option<Self>, OfferedFault> {
        let path = dir.offered(&Slug::parse(id).map_err(|_| OfferedFault::Name)?);
        let text = read_optional(&path).map_err(|_| OfferedFault::Io)?;
        text.map(|t| toml::from_str(&t).map_err(|_| OfferedFault::Damaged))
            .transpose()
    }

    /// Replaces the record of `id` with `self`, its counter one past the one it replaces. A start
    /// that did not get as far as listing models keeps the models listed before.
    pub fn write(self, dir: &AgentsDir, id: &str) -> Result<Self, OfferedFault> {
        let path = dir.offered(&Slug::parse(id).map_err(|_| OfferedFault::Name)?);
        let before = Self::read(dir, id).ok().flatten();
        let seen = before.as_ref().map_or(0, |o| o.seen) + 1;
        let kept = |o: &Self| (o.in_use.clone(), o.models.clone());
        let (in_use, models) = match (self.models.is_empty(), &before) {
            (true, Some(old)) if self.start != Start::SignedIn => kept(old),
            _ => (self.in_use.clone(), self.models.clone()),
        };
        let next = Self {
            seen,
            in_use,
            models,
            ..self
        };
        let text = toml::to_string(&next).map_err(|_| OfferedFault::Damaged)?;
        write_atomic(&path, text.as_bytes()).map_err(|_| OfferedFault::Io)?;
        Ok(next)
    }
}
