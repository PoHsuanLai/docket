//! What the board writes down and reads back after a restart: one record per change of a card, a
//! job or a post, each the whole thing as it now stands (the newest wins). They are stored as
//! `Area { Helpers }` payloads of kinds `helpers.card`, `helpers.job` and `helpers.post`, trusted
//! and private to the helper's Space. A record holds no text a model wrote: a job's goal is the
//! person's words or a planner's that read nothing untrusted, and an answer is never copied.

use super::card::HelperCard;
use super::job::Job;
use super::post::Post;
use prov::HelperId;
use serde::{Deserialize, Serialize};

/// Which kind of record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HelperRecordKind {
    /// A card.
    Card,
    /// A job.
    Job,
    /// A post.
    Post,
}

impl HelperRecordKind {
    /// Every kind, for a query that reads them all.
    pub const ALL: [HelperRecordKind; 3] = [
        HelperRecordKind::Card,
        HelperRecordKind::Job,
        HelperRecordKind::Post,
    ];

    /// The record's kind tag in the event log.
    pub fn slug(self) -> &'static str {
        match self {
            HelperRecordKind::Card => "helpers.card",
            HelperRecordKind::Job => "helpers.job",
            HelperRecordKind::Post => "helpers.post",
        }
    }

    /// The kind a tag names.
    pub fn of_slug(slug: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.slug() == slug)
    }
}

/// A card's record: kept as it now stands, or gone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum CardRecord {
    /// The card.
    Kept(Box<HelperCard>),
    /// The person removed it.
    Removed(HelperId),
}

/// One record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum HelperRecord {
    /// A card.
    Card(CardRecord),
    /// A job.
    Job(Box<Job>),
    /// A post.
    Post(Box<Post>),
}

impl HelperRecord {
    /// Which kind it is.
    pub fn kind(&self) -> HelperRecordKind {
        match self {
            HelperRecord::Card(_) => HelperRecordKind::Card,
            HelperRecord::Job(_) => HelperRecordKind::Job,
            HelperRecord::Post(_) => HelperRecordKind::Post,
        }
    }

    /// The payload as it is stored: the inner value's serde form, not the enum's.
    pub fn json(&self) -> Result<String, serde_json::Error> {
        match self {
            HelperRecord::Card(card) => serde_json::to_string(card),
            HelperRecord::Job(job) => serde_json::to_string(job),
            HelperRecord::Post(post) => serde_json::to_string(post),
        }
    }

    /// The record of `kind` whose payload is `text`.
    pub fn from_json(kind: HelperRecordKind, text: &str) -> Result<Self, serde_json::Error> {
        Ok(match kind {
            HelperRecordKind::Card => HelperRecord::Card(serde_json::from_str(text)?),
            HelperRecordKind::Job => HelperRecord::Job(Box::new(serde_json::from_str(text)?)),
            HelperRecordKind::Post => HelperRecord::Post(Box::new(serde_json::from_str(text)?)),
        })
    }
}
