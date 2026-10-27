//! Skill directories on the real disk, the client/IO side of `docket-skills`.
//!
//! `docket-skills` does no IO: it asks a [`SkillFolder`] to list and read. [`DiskFolder`] is that
//! folder over `std::fs`. It reads only the paths it is handed and names no environment variable.

use docket_skills::{FolderError, SkillFolder};
use std::path::{Path, PathBuf};

/// The real disk, as a [`SkillFolder`].
#[derive(Debug, Clone, Copy, Default)]
pub struct DiskFolder;

impl SkillFolder for DiskFolder {
    fn subdirs(&self, root: &Path) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::fs::read_dir(root)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        dirs
    }

    fn read(&self, path: &Path) -> Result<String, FolderError> {
        std::fs::read_to_string(path).map_err(|e| FolderError { why: e.to_string() })
    }
}
