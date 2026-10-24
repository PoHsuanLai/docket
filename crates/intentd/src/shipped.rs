//! The entries an app ships in `quire/search/<AppName>.toml`: fixed things the launcher finds
//! without the app running (Settings' pages, say). Parsing only; `search_files` reads
//! the files.

use docket_core::IndexEntry;
use prov::{EntityKey, EntityKind, SpaceScope, UnixSeconds};
use serde::Deserialize;

/// Why a shipped file is not taken.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ShippedFault {
    /// The text is not TOML of the shipped shape, or a kind or key is not a valid id.
    #[error("not a search file: {0}")]
    Malformed(String),
}

/// One `[[entries]]` table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    kind: EntityKind,
    key: EntityKey,
    title: String,
    #[serde(default)]
    subtitle: String,
    #[serde(default)]
    keywords: Vec<String>,
}

/// The whole file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    entries: Vec<Row>,
}

/// The entries of one shipped file, as index entries in no Space in particular.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShippedEntries(Vec<IndexEntry>);

impl ShippedEntries {
    /// Reads the text of a shipped file.
    pub fn parse(text: &str) -> Result<Self, ShippedFault> {
        let file: File =
            toml::from_str(text).map_err(|e| ShippedFault::Malformed(e.to_string()))?;
        Ok(Self(
            file.entries
                .into_iter()
                .map(|row| IndexEntry {
                    key: row.key,
                    kind: row.kind,
                    title: row.title,
                    subtitle: row.subtitle,
                    keywords: row.keywords,
                    updated: UnixSeconds(0),
                    space: SpaceScope::Any,
                })
                .collect(),
        ))
    }

    /// The entries, in file order.
    pub fn into_entries(self) -> Vec<IndexEntry> {
        self.0
    }
}
