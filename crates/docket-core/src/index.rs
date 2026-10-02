//! The shadow index: apps push entity titles so the launcher can search without waking them.
//! The owner is derived from the connection and must equal the manifest's app.

use crate::context::EntityRef;
use crate::ids::LabelText;
use crate::units::Generation;
use prov::{EntityKey, EntityKind, SpaceScope};
use serde::{Deserialize, Serialize};

/// One indexed thing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// Its key in the app.
    pub key: EntityKey,
    /// Its kind.
    pub kind: EntityKind,
    /// Its title. Apps push plain text and never a label: the router labels it from the kind's
    /// `TitleTrust`.
    pub title: String,
    /// Its subtitle.
    pub subtitle: String,
    /// Words that find it.
    pub keywords: Vec<String>,
    /// When it last changed.
    pub updated: prov::UnixSeconds,
    /// Which Spaces may find it.
    pub space: SpaceScope,
}

/// One push: a batch of upserts and removals in an epoch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexBatch {
    /// The epoch the app syncs in.
    pub epoch: u64,
    /// Things added or changed.
    pub upserts: Vec<IndexEntry>,
    /// Things removed.
    pub removals: Vec<EntityKey>,
}

/// Where an app's index stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum IndexState {
    /// Nothing known.
    Unknown,
    /// Receiving this epoch.
    Syncing(u64),
    /// Complete for this epoch.
    Synced(u64),
}

/// A search hit.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hit {
    /// The thing.
    pub entity: EntityRef,
    /// Why it was found, in the app's words.
    pub why: Option<LabelText>,
}

/// What a search covers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SearchScope {
    /// Everything the caller may see.
    Everything,
    /// One kind.
    Kind(EntityKind),
}

/// A search in flight; a newer generation supersedes an older.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchAsk {
    /// The words.
    pub text: String,
    /// What to cover.
    pub scope: SearchScope,
    /// Its generation.
    pub generation: Generation,
}

/// Options for a parameter that is offered by the app (`ParamType::Dynamic`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SuggestAsk {
    /// The action.
    pub action: crate::ids::ActionRef,
    /// The parameter.
    pub param: crate::ids::ParamName,
    /// What the person typed so far.
    pub typed: String,
}
