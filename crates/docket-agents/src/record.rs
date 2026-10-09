//! What was installed and how it was checked, kept beside the files.

use crate::hash::Sha256Hex;
use crate::slug::Slug;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// How the install was checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Check {
    /// The registry gave the archive's digest and it matched.
    Declared,
    /// The registry gave none: the digest of what arrived is kept, and was never compared.
    FirstUse,
    /// The package manager checked its own packages.
    PackageManager,
}

/// One installed version.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    /// The registry id.
    pub id: String,
    /// The version.
    pub version: String,
    /// The program, relative to `files/`.
    pub command: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Plain variables the registry says it wants.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// The archive's digest, when there was an archive.
    pub digest: Option<String>,
    /// How it was checked.
    pub check: Check,
}

/// Why a record was not read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RecordFault {
    /// Nothing installed there.
    #[error("that agent is not installed")]
    Missing,
    /// The record is not one, or names a program outside its directory.
    #[error("the install record is damaged")]
    Damaged,
}

/// The file name of the record.
pub const FILE: &str = "installed.toml";
/// The directory the files are in.
pub const FILES: &str = "files";

impl Record {
    /// Writes the record into `dir`.
    pub fn write(&self, dir: &Path) -> std::io::Result<()> {
        let text = toml::to_string(self).map_err(std::io::Error::other)?;
        std::fs::write(dir.join(FILE), text)
    }

    /// Reads the record in `dir`.
    pub fn read(dir: &Path) -> Result<Self, RecordFault> {
        let text = std::fs::read_to_string(dir.join(FILE)).map_err(|_| RecordFault::Missing)?;
        let record: Self = toml::from_str(&text).map_err(|_| RecordFault::Damaged)?;
        let named = Slug::parse(&record.id).is_ok() && Slug::parse(&record.version).is_ok();
        let stays = Path::new(&record.command)
            .components()
            .all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        let digest = record
            .digest
            .as_deref()
            .is_none_or(|d| Sha256Hex::parse(d).is_ok());
        (named && stays && digest)
            .then_some(record)
            .ok_or(RecordFault::Damaged)
    }

    /// The program's path, given the version's directory.
    pub fn command_in(&self, dir: &Path) -> PathBuf {
        dir.join(FILES).join(&self.command)
    }
}
