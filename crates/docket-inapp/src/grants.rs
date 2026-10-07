//! The person's standing consent in a file the app names, so "always" survives a restart.
//!
//! The format is intentd's own (`grants.json`: a JSON list of grants), so a file written here is
//! one the desktop's `FileGrants` reads. Nothing here finds a directory: the app gives the path
//! (its own data directory), and no `/proc`, XDG lookup or platform call is made. A write goes
//! through a temporary file beside the target and a rename, so a crash leaves the old file or
//! the new one, never half of one. The `GrantStore` seam has no way to say "failed" (the router
//! must not stop a call over a disk), so a failed write is kept for the app to ask for
//! ([`FileGrantStore::take_fault`]); the grant still counts for this run.

use docket_core::ActionGrant;
use docket_router::GrantStore;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard, PoisonError};

/// Why the grant file could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GrantFileError {
    /// The file exists and could not be read.
    #[error("cannot read {path}: {kind}")]
    Read {
        /// The file.
        path: PathBuf,
        /// What the system said.
        kind: ErrorKind,
    },
    /// The file is not a list of grants. The app decides: start over ([`FileGrantStore::fresh`])
    /// or ask the person.
    #[error("{path} is not a list of grants: {why}")]
    Corrupt {
        /// The file.
        path: PathBuf,
        /// What the parser said.
        why: String,
    },
    /// The file (or its temporary) could not be written.
    #[error("cannot write {path}: {kind}")]
    Write {
        /// The file.
        path: PathBuf,
        /// What the system said.
        kind: ErrorKind,
    },
}

/// The grants a file's text holds: an empty text is no grants.
pub fn decode(text: &str) -> Result<Vec<ActionGrant>, String> {
    match text.trim() {
        "" => Ok(Vec::new()),
        json => serde_json::from_str(json).map_err(|why| why.to_string()),
    }
}

/// The text of a list of grants.
pub fn encode(grants: &[ActionGrant]) -> String {
    serde_json::to_string_pretty(grants).unwrap_or_else(|_| "[]".to_owned())
}

/// `grants` with `grant` recorded: a grant for the same key replaces the earlier one.
pub fn recorded(mut grants: Vec<ActionGrant>, grant: ActionGrant) -> Vec<ActionGrant> {
    grants.retain(|held| held.key != grant.key);
    grants.push(grant);
    grants
}

fn read_file(path: &Path) -> Result<Vec<ActionGrant>, GrantFileError> {
    match std::fs::read_to_string(path) {
        Ok(text) => decode(&text).map_err(|why| GrantFileError::Corrupt {
            path: path.to_owned(),
            why,
        }),
        Err(why) if why.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(why) => Err(GrantFileError::Read {
            path: path.to_owned(),
            kind: why.kind(),
        }),
    }
}

fn write_file(path: &Path, grants: &[ActionGrant]) -> Result<(), GrantFileError> {
    use std::io::Write;
    let failed = |why: std::io::Error| GrantFileError::Write {
        path: path.to_owned(),
        kind: why.kind(),
    };
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".tmp");
    let temporary = path.with_file_name(name);
    let mut file = std::fs::File::create(&temporary).map_err(failed)?;
    file.write_all(encode(grants).as_bytes()).map_err(failed)?;
    file.sync_all().map_err(failed)?;
    std::fs::rename(&temporary, path).map_err(failed)
}

#[derive(Debug, Default)]
struct Held {
    grants: Vec<ActionGrant>,
    fault: Option<GrantFileError>,
}

/// Grants kept in the file at the path the app gave. Two stores on one file see each other's
/// grants: a read goes to the file, and a record merges into what the file holds.
#[derive(Debug)]
pub struct FileGrantStore {
    path: PathBuf,
    held: Mutex<Held>,
}

impl FileGrantStore {
    /// The store for `path`, with what the file holds now (nothing when it is missing).
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, GrantFileError> {
        let path = path.into();
        let grants = read_file(&path)?;
        Ok(Self {
            path,
            held: Mutex::new(Held {
                grants,
                fault: None,
            }),
        })
    }

    /// The store for `path` that does not fail on a damaged file: it holds nothing while the file
    /// is unreadable, and the first grant recorded replaces it.
    pub fn fresh(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            held: Mutex::default(),
        }
    }

    /// The file this store keeps.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The last write that failed, if one did; asking clears it.
    pub fn take_fault(&self) -> Option<GrantFileError> {
        self.locked().fault.take()
    }

    fn locked(&self) -> MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl GrantStore for FileGrantStore {
    fn grants(&self) -> Vec<ActionGrant> {
        let mut held = self.locked();
        if let Ok(on_disk) = read_file(&self.path) {
            held.grants = on_disk;
        }
        held.grants.clone()
    }

    fn record(&self, grant: ActionGrant) {
        let mut held = self.locked();
        let current = read_file(&self.path).unwrap_or_else(|_| held.grants.clone());
        held.grants = recorded(current, grant);
        held.fault = write_file(&self.path, &held.grants).err();
    }
}
