//! The seam to the disk: how a skill directory is listed and read. The caller supplies the
//! implementation (`docket-store` has the one over the real disk), so this crate does no IO.

use std::path::{Path, PathBuf};

/// A file that could not be read, in the reader's words.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{why}")]
pub struct FolderError {
    /// What went wrong.
    pub why: String,
}

/// Reads skill directories.
pub trait SkillFolder {
    /// The directories directly under `root`, sorted by path. A root that is missing or cannot be
    /// listed has none.
    fn subdirs(&self, root: &Path) -> Vec<PathBuf>;

    /// The text of the file at `path`.
    fn read(&self, path: &Path) -> Result<String, FolderError>;
}
