//! A registry snapshot, as the registry publishes it. Docket ships no agent list of its own: what
//! can be installed is whatever the snapshot in hand says.

use crate::platform::Platform;
use serde::Deserialize;
use std::collections::BTreeMap;

/// The whole snapshot. Fields docket has no use for are ignored.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Snapshot {
    /// The agents on offer.
    pub agents: Vec<Listing>,
}

/// One agent in the snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Listing {
    /// Its id in the registry (`antigravity-acp`).
    pub id: String,
    /// What the registry calls it.
    pub name: String,
    /// The version on offer.
    pub version: String,
    /// How to get it.
    pub distribution: Distribution,
}

/// The ways an agent is distributed; an agent may offer several.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Distribution {
    /// A prebuilt program per platform.
    #[serde(default)]
    pub binary: BTreeMap<Platform, Binary>,
    /// A package from the node package registry.
    pub npx: Option<Package>,
    /// A package from the python package index, installed with `uv`.
    pub uvx: Option<Package>,
}

/// A prebuilt program in an archive.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Binary {
    /// Where the archive is.
    pub archive: String,
    /// The program inside it, relative (`./goose`).
    pub cmd: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Plain variables it wants set.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The archive's SHA-256, when the registry gives one.
    pub sha256: Option<String>,
}

/// A package and what it runs with.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Package {
    /// The package with its version (`@scope/name@1.2.3`).
    pub package: String,
    /// Extra arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Plain variables it wants set.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
}

/// Why a snapshot was not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("the agent list is not valid: {0}")]
pub struct SnapshotFault(String);

impl Snapshot {
    /// Reads the snapshot's JSON.
    pub fn parse(text: &str) -> Result<Self, SnapshotFault> {
        serde_json::from_str(text).map_err(|e| SnapshotFault(e.to_string()))
    }

    /// The agent with `id`.
    pub fn find(&self, id: &str) -> Option<&Listing> {
        self.agents.iter().find(|a| a.id == id)
    }
}
