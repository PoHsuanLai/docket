//! Reading and writing a consent file without ever losing it.
//!
//! A file that is missing holds no grants. A file that cannot be read, or is not a list of
//! grants, is a [`GrantFileFault`]: the caller logs it and leaves the file alone, because
//! treating it as empty would let the next write replace every grant the person gave.

use docket_core::{read_optional, write_atomic};
use std::fmt::Display;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

/// Why a consent file could not be read or written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GrantFileFault {
    /// The file exists and could not be read.
    #[error("cannot read {path}: {kind}")]
    Read { path: PathBuf, kind: ErrorKind },
    /// The file is not a list of grants.
    #[error("{path} is not a list of grants: {why}")]
    Corrupt { path: PathBuf, why: String },
    /// The grants could not be turned into text, so the file is left as it was.
    #[error("grants for {path} could not be encoded: {why}")]
    Encode { path: PathBuf, why: String },
    /// The file could not be replaced; it holds what it held before.
    #[error("cannot write {path}: {kind}")]
    Write { path: PathBuf, kind: ErrorKind },
}

/// What the file at `path` holds, decoded: `none` when the file is missing or blank.
pub(crate) fn read_with<T, E: Display>(
    path: &Path,
    none: T,
    decode: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, GrantFileFault> {
    let read = read_optional(path).map_err(|why| GrantFileFault::Read {
        path: path.to_owned(),
        kind: why.kind(),
    })?;
    match read.as_deref().map(str::trim) {
        None | Some("") => Ok(none),
        Some(text) => decode(text).map_err(|why| GrantFileFault::Corrupt {
            path: path.to_owned(),
            why: why.to_string(),
        }),
    }
}

/// The list the file at `path` holds: none when the file is missing or blank.
pub(crate) fn read_list<T, E: Display>(
    path: &Path,
    decode: impl FnOnce(&str) -> Result<Vec<T>, E>,
) -> Result<Vec<T>, GrantFileFault> {
    read_with(path, Vec::new(), decode)
}

/// Replaces the file at `path` with `text`. A text that could not be encoded writes nothing.
pub(crate) fn write_list<E: Display>(
    path: &Path,
    text: Result<String, E>,
) -> Result<(), GrantFileFault> {
    let text = text.map_err(|why| GrantFileFault::Encode {
        path: path.to_owned(),
        why: why.to_string(),
    })?;
    write_atomic(path, text.as_bytes()).map_err(|why| GrantFileFault::Write {
        path: path.to_owned(),
        kind: why.kind(),
    })
}
