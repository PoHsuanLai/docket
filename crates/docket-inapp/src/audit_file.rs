//! The audit records waiting for memory, kept in a file the app names so they survive a restart.
//!
//! The queue the router fills is bounded and lives in memory; records memory could not take
//! (it is away, locked or busy) wait in it for the next flush. Quit the app in that state and they
//! were lost. With a file the agent writes the waiting records after each flush, and the next
//! agent reads them back into its queue, to be written on its first flush.
//!
//! Nothing here finds a directory: the app gives the path. A write goes through a temporary file
//! beside the target and a rename, so a crash leaves the old file or the new one, never half of
//! one; the file holds at most `limit` records (the newest), so it cannot grow without end; an
//! empty queue removes the file. A write that fails is kept for the app to ask for
//! ([`InAppAgent::take_audit_fault`](crate::InAppAgent)): the records still wait in memory for
//! this run.

use docket_core::AuditRecord;
use docket_memory::QUEUE_LIMIT;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Why the file could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuditFileError {
    /// The file exists and could not be read.
    #[error("cannot read {path}: {kind}")]
    Read {
        /// The file.
        path: PathBuf,
        /// What the system said.
        kind: ErrorKind,
    },
    /// The file is not a list of audit records. The app decides: start over
    /// ([`AuditFile::fresh`]) or keep the file for the person.
    #[error("{path} is not a list of audit records: {why}")]
    Corrupt {
        /// The file.
        path: PathBuf,
        /// What the parser said.
        why: String,
    },
    /// The file (or its temporary) could not be written or removed.
    #[error("cannot write {path}: {kind}")]
    Write {
        /// The file.
        path: PathBuf,
        /// What the system said.
        kind: ErrorKind,
    },
}

/// The records a file's text holds: an empty text is none.
pub fn decode(text: &str) -> Result<Vec<AuditRecord>, String> {
    match text.trim() {
        "" => Ok(Vec::new()),
        json => serde_json::from_str(json).map_err(|why| why.to_string()),
    }
}

/// The text of a list of records, the newest `limit` of them.
pub fn encode(records: &[AuditRecord], limit: usize) -> String {
    let skip = records.len().saturating_sub(limit);
    serde_json::to_string(&records[skip..]).unwrap_or_else(|_| "[]".to_owned())
}

/// The queue file at a path, and the records it held when it was opened.
#[derive(Debug, Clone, PartialEq)]
pub struct AuditFile {
    path: PathBuf,
    limit: usize,
    waiting: Vec<AuditRecord>,
}

impl AuditFile {
    /// The file at `path`, with what it holds now (nothing when it is missing).
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AuditFileError> {
        let path = path.into();
        let waiting = read_file(&path)?;
        Ok(Self {
            path,
            limit: QUEUE_LIMIT,
            waiting,
        })
    }

    /// The file at `path` that does not fail on a damaged file: it holds nothing, and the first
    /// save replaces it.
    pub fn fresh(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            limit: QUEUE_LIMIT,
            waiting: Vec::new(),
        }
    }

    /// The file keeps at most `limit` records.
    pub fn bounded(self, limit: usize) -> Self {
        Self { limit, ..self }
    }

    /// The path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What the file held when it was opened, oldest first.
    pub fn waiting(&self) -> &[AuditRecord] {
        &self.waiting
    }

    /// Writes `records` (the queue as it stands) as the file's content.
    pub fn save(&self, records: &[AuditRecord]) -> Result<(), AuditFileError> {
        write_file(&self.path, records, self.limit)
    }
}

fn read_file(path: &Path) -> Result<Vec<AuditRecord>, AuditFileError> {
    match std::fs::read_to_string(path) {
        Ok(text) => decode(&text).map_err(|why| AuditFileError::Corrupt {
            path: path.to_owned(),
            why,
        }),
        Err(why) if why.kind() == ErrorKind::NotFound => Ok(Vec::new()),
        Err(why) => Err(AuditFileError::Read {
            path: path.to_owned(),
            kind: why.kind(),
        }),
    }
}

fn write_file(path: &Path, records: &[AuditRecord], limit: usize) -> Result<(), AuditFileError> {
    use std::io::Write;
    let failed = |why: std::io::Error| AuditFileError::Write {
        path: path.to_owned(),
        kind: why.kind(),
    };
    if records.is_empty() {
        return match std::fs::remove_file(path) {
            Err(why) if why.kind() != ErrorKind::NotFound => Err(failed(why)),
            _ => Ok(()),
        };
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(".tmp");
    let temporary = path.with_file_name(name);
    let mut file = std::fs::File::create(&temporary).map_err(failed)?;
    file.write_all(encode(records, limit).as_bytes())
        .map_err(failed)?;
    file.sync_all().map_err(failed)?;
    std::fs::rename(&temporary, path).map_err(failed)
}
