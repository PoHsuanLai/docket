//! The one confined-path type for a directory docket hands to a sandbox as writable: a terminal's
//! working directory, and the agent's own. A [`Workdir`] is a real directory (links resolved)
//! that lies inside the session's folder, that no link on the way led to (the agent can make
//! links in the folder it writes), that is not the person's home and does not hold it, and that
//! is not a place of secrets (the same names `fs/*` refuses).
//!
//! `resolve` walks the file system; `from_resolved` is the pure half, given the real paths.

use super::confine::is_secret;
use docket_core::{AbsPath, Cover};
use std::path::{Path, PathBuf};

/// Why a directory is not a [`Workdir`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WorkdirFault {
    /// Outside the session's folder.
    #[error("outside the session's folder")]
    Outside,
    /// A link on the way: a plain folder is needed.
    #[error("a link on the way to the folder")]
    Link,
    /// Not there, or not a folder.
    #[error("not a folder")]
    NotFolder,
    /// The person's home itself, or a folder that holds it.
    #[error("the home folder cannot be a workspace; choose a folder inside it")]
    Home,
    /// A place that holds the person's secrets.
    #[error("a place that holds secrets")]
    Secret,
    /// What the path leads to changed while it was being opened.
    #[error("the folder changed while it was opened")]
    Moved,
}

/// A directory fit to be writable inside a sandbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workdir(AbsPath);

fn abs(path: &Path) -> Result<AbsPath, WorkdirFault> {
    AbsPath::try_from(path).map_err(|_| WorkdirFault::NotFolder)
}

impl Workdir {
    /// The real directory.
    pub fn real(&self) -> &AbsPath {
        &self.0
    }

    /// The pure rules, given `real_scope` and `real` (both links resolved) and the person's
    /// `home` if known.
    pub fn from_resolved(
        real_scope: &AbsPath,
        real: AbsPath,
        home: Option<&AbsPath>,
    ) -> Result<Self, WorkdirFault> {
        if real_scope.covers(&real) != Cover::Covers {
            return Err(WorkdirFault::Outside);
        }
        // Home itself, or a folder above it, would make home (and its dot-directories) writable.
        if home.is_some_and(|h| real.covers(h) == Cover::Covers) {
            return Err(WorkdirFault::Home);
        }
        if is_secret(&real) {
            return Err(WorkdirFault::Secret);
        }
        Ok(Self(real))
    }

    /// `asked`, which must lie inside `scope`, as a confined directory. Every part below `scope`
    /// is looked at without following links, and any link refuses; `scope` itself is resolved.
    /// The opened directory is compared with the one that was looked at.
    pub fn resolve(
        scope: &AbsPath,
        asked: &AbsPath,
        home: Option<&Path>,
    ) -> Result<Self, WorkdirFault> {
        if scope.covers(asked) != Cover::Covers {
            return Err(WorkdirFault::Outside);
        }
        let real_scope =
            std::fs::canonicalize(scope.as_str()).map_err(|_| WorkdirFault::NotFolder)?;
        let below = asked
            .as_str()
            .strip_prefix(scope.as_str())
            .unwrap_or_default();
        let mut at: PathBuf = real_scope.clone();
        for part in below.split('/').filter(|p| !p.is_empty()) {
            at.push(part);
            let meta = std::fs::symlink_metadata(&at).map_err(|_| WorkdirFault::NotFolder)?;
            if meta.file_type().is_symlink() {
                return Err(WorkdirFault::Link);
            }
        }
        let looked = std::fs::symlink_metadata(&at).map_err(|_| WorkdirFault::NotFolder)?;
        if !looked.is_dir() {
            return Err(WorkdirFault::NotFolder);
        }
        let opened = std::fs::File::open(&at)
            .and_then(|f| f.metadata())
            .map_err(|_| WorkdirFault::NotFolder)?;
        if !same_directory(&looked, &opened) {
            return Err(WorkdirFault::Moved);
        }
        let home = home
            .map(|h| std::fs::canonicalize(h).unwrap_or_else(|_| h.to_path_buf()))
            .and_then(|h| abs(&h).ok());
        Self::from_resolved(&abs(&real_scope)?, abs(&at)?, home.as_ref())
    }
}

#[cfg(unix)]
fn same_directory(a: &std::fs::Metadata, b: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    (a.dev(), a.ino()) == (b.dev(), b.ino())
}

#[cfg(not(unix))]
fn same_directory(_: &std::fs::Metadata, _: &std::fs::Metadata) -> bool {
    false
}
