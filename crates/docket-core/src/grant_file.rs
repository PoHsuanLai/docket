//! Reading and writing a consent file without ever losing it. The one place the grant-file
//! format's disk rules live; the daemon and an app's in-process store both use it.
//!
//! A file that is missing or blank holds no grants. A file that cannot be read, or is not a list
//! of grants, is a [`GrantFileError`]: the caller keeps the file and says why, because treating it
//! as empty would let the next write replace every grant the person gave.

use crate::atomic_file::{read_optional, write_atomic};
use std::fmt::Display;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Why a consent file could not be read or written.
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
    /// The file is not a list of grants. The host decides: start over or ask the person.
    #[error("{path} is not a list of grants: {why}")]
    Corrupt {
        /// The file.
        path: PathBuf,
        /// What the parser said.
        why: String,
    },
    /// The grants could not be turned into text, so the file is left as it was.
    #[error("grants for {path} could not be encoded: {why}")]
    Encode {
        /// The file.
        path: PathBuf,
        /// What the encoder said.
        why: String,
    },
    /// The file could not be replaced; it holds what it held before.
    #[error("cannot write {path}: {kind}")]
    Write {
        /// The file.
        path: PathBuf,
        /// What the system said.
        kind: ErrorKind,
    },
}

/// What the file at `path` holds, decoded: `none` when the file is missing or blank.
pub fn read_grant_file<T, E: Display>(
    path: &Path,
    none: T,
    decode: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, GrantFileError> {
    let read = read_optional(path).map_err(|why| GrantFileError::Read {
        path: path.to_owned(),
        kind: why.kind(),
    })?;
    match read.as_deref().map(str::trim) {
        None | Some("") => Ok(none),
        Some(text) => decode(text).map_err(|why| GrantFileError::Corrupt {
            path: path.to_owned(),
            why: why.to_string(),
        }),
    }
}

/// The list the file at `path` holds: none when the file is missing or blank.
pub fn read_grant_list<T, E: Display>(
    path: &Path,
    decode: impl FnOnce(&str) -> Result<Vec<T>, E>,
) -> Result<Vec<T>, GrantFileError> {
    read_grant_file(path, Vec::new(), decode)
}

/// Replaces the file at `path` with `text`. A text that could not be encoded writes nothing.
pub fn write_grant_list<E: Display>(
    path: &Path,
    text: Result<String, E>,
) -> Result<(), GrantFileError> {
    let text = text.map_err(|why| GrantFileError::Encode {
        path: path.to_owned(),
        why: why.to_string(),
    })?;
    write_atomic(path, text.as_bytes()).map_err(|why| GrantFileError::Write {
        path: path.to_owned(),
        kind: why.kind(),
    })
}
