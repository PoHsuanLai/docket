//! Where docket keeps agents: one directory it owns, apart from the person's own files.
//!
//! ```text
//! <root>/installed/<id>/<version>/files/        what the archive unpacked to
//! <root>/installed/<id>/<version>/installed.toml  what was installed and how it was checked
//! <root>/state/<id>/home/                         the agent's own home inside its sandbox
//! <root>/state/<id>/offered.toml                  what it last offered (see `offered`)
//! ```
//!
//! The agent signs in inside `home` and keeps its login there. Nothing is copied into it from the
//! person's real home.

use crate::record::Record;
use crate::slug::Slug;
use std::path::{Path, PathBuf};

/// The agents directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentsDir(PathBuf);

impl AgentsDir {
    /// The directory at `root` (made when something is installed).
    pub fn at(root: &Path) -> Self {
        Self(root.to_owned())
    }

    /// Where version `version` of `id` is installed.
    pub fn installed(&self, id: &Slug, version: &Slug) -> PathBuf {
        self.0
            .join("installed")
            .join(id.as_str())
            .join(version.as_str())
    }

    /// The versions of `id` that are installed.
    pub fn versions(&self, id: &Slug) -> Vec<Slug> {
        let dir = self.0.join("installed").join(id.as_str());
        let Ok(read) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut found: Vec<Slug> = read
            .filter_map(|e| {
                let slug = e
                    .ok()?
                    .file_name()
                    .to_str()
                    .and_then(|n| Slug::parse(n).ok())?;
                // Only a version with its record is installed; a staging directory has none.
                Record::read(&dir.join(slug.as_str())).ok().map(|_| slug)
            })
            .collect();
        found.sort();
        found
    }

    /// The agent's home inside its sandbox.
    pub fn home(&self, id: &Slug) -> PathBuf {
        self.0.join("state").join(id.as_str()).join("home")
    }

    /// The record of what `id` last offered.
    pub fn offered(&self, id: &Slug) -> PathBuf {
        self.0
            .join("state")
            .join(id.as_str())
            .join(crate::offered::FILE)
    }
}
