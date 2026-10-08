//! The file system the agent's `fs/*` calls reach, behind a seam. `OsFiles` is the real one, std
//! only; a test uses `FakeFiles` (in `fake`). The seam answers three things: where a path really
//! leads (links followed), a bounded read, and a write that hands back what it replaced so the
//! undo journal can keep it.
//!
//! `OsFiles` checks a second time after it opens: the descriptor's own target (`/proc/self/fd`)
//! must still lie inside the session's real directory, so a link swapped in between the check and
//! the open (the agent can write in its directory and make links) is caught before any byte is
//! read or changed.

use docket_core::{AbsPath, Cover};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

/// The most a read returns.
pub const MAX_READ: usize = 2 * 1024 * 1024;
/// The most a write accepts.
pub const MAX_WRITE: usize = 4 * 1024 * 1024;

/// Why a file call failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum FileFault {
    /// There is no such file.
    #[error("no such file")]
    NotFound,
    /// Too large to read or write through ACP.
    #[error("the file is too large")]
    TooBig,
    /// Not UTF-8 text.
    #[error("the file is not text")]
    NotText,
    /// The file system refused.
    #[error("the file system refused")]
    Refused,
    /// What the path leads to changed between the check and the open.
    #[error("the path changed under the call")]
    Moved,
}

/// The seam.
pub trait Files: Send {
    /// Where `path` really leads, links followed. A missing last part is allowed (a new file):
    /// the nearest existing parent is resolved and the rest appended.
    fn real(&self, path: &AbsPath) -> Result<AbsPath, FileFault>;

    /// The text of the file at `real`, which must still lie inside `within`.
    fn read(&mut self, real: &AbsPath, within: &AbsPath) -> Result<String, FileFault>;

    /// Replaces (or creates) the file at `real`, which must still lie inside `within`. Returns
    /// the text it replaced, `None` for a new file.
    fn write(
        &mut self,
        real: &AbsPath,
        within: &AbsPath,
        content: &str,
    ) -> Result<Option<String>, FileFault>;

    /// Removes the file at `real`, which must still lie inside `within`: what undoing a write
    /// that created the file does.
    fn remove(&mut self, real: &AbsPath, within: &AbsPath) -> Result<(), FileFault>;
}

/// The real file system.
#[derive(Debug, Clone, Copy, Default)]
pub struct OsFiles;

fn abs(path: &Path) -> Result<AbsPath, FileFault> {
    path.to_str()
        .and_then(|t| AbsPath::parse(t).ok())
        .ok_or(FileFault::Refused)
}

fn fault(error: &std::io::Error) -> FileFault {
    match error.kind() {
        std::io::ErrorKind::NotFound => FileFault::NotFound,
        _ => FileFault::Refused,
    }
}

/// Resolves the deepest existing ancestor of `path` and appends the rest.
fn resolve(path: &Path) -> Result<PathBuf, FileFault> {
    let mut rest = Vec::new();
    let mut here = path.to_path_buf();
    loop {
        match here.canonicalize() {
            Ok(real) => {
                return Ok(rest.iter().rev().fold(real, |acc, part| acc.join(part)));
            }
            Err(_) => {
                let name = here.file_name().map(ToOwned::to_owned);
                let (Some(name), Some(parent)) = (name, here.parent()) else {
                    return Err(FileFault::NotFound);
                };
                rest.push(name);
                here = parent.to_path_buf();
            }
        }
    }
}

/// Whether the open descriptor still points inside `within`.
#[cfg(unix)]
fn still_inside(file: &File, within: &AbsPath) -> Result<(), FileFault> {
    use std::os::fd::AsRawFd;
    let target = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))
        .map_err(|_| FileFault::Moved)?;
    let target = abs(&target).map_err(|_| FileFault::Moved)?;
    if within.covers(&target) == Cover::Covers {
        Ok(())
    } else {
        Err(FileFault::Moved)
    }
}

/// Where there is no `/proc/self/fd`, the check cannot be made, so the call is refused.
#[cfg(not(unix))]
fn still_inside(_file: &File, _within: &AbsPath) -> Result<(), FileFault> {
    Err(FileFault::Moved)
}

fn read_text(file: &mut File) -> Result<String, FileFault> {
    let mut bytes = Vec::new();
    Read::by_ref(file)
        .take(MAX_READ as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| fault(&e))?;
    if bytes.len() > MAX_READ {
        return Err(FileFault::TooBig);
    }
    String::from_utf8(bytes).map_err(|_| FileFault::NotText)
}

impl Files for OsFiles {
    fn real(&self, path: &AbsPath) -> Result<AbsPath, FileFault> {
        let resolved = resolve(Path::new(path.as_str()))?;
        // `Component` keeps only normal parts: a resolved path has no `..` left.
        debug_assert!(
            resolved
                .components()
                .all(|c| !matches!(c, Component::ParentDir))
        );
        abs(&resolved)
    }

    fn read(&mut self, real: &AbsPath, within: &AbsPath) -> Result<String, FileFault> {
        let mut file = File::open(real.as_str()).map_err(|e| fault(&e))?;
        still_inside(&file, within)?;
        if file.metadata().map_err(|e| fault(&e))?.is_dir() {
            return Err(FileFault::Refused);
        }
        read_text(&mut file)
    }

    fn write(
        &mut self,
        real: &AbsPath,
        within: &AbsPath,
        content: &str,
    ) -> Result<Option<String>, FileFault> {
        if content.len() > MAX_WRITE {
            return Err(FileFault::TooBig);
        }
        let existed = Path::new(real.as_str()).exists();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(real.as_str())
            .map_err(|e| fault(&e))?;
        still_inside(&file, within)?;
        let before = if existed {
            Some(read_text(&mut file)?)
        } else {
            None
        };
        file.set_len(0).map_err(|e| fault(&e))?;
        // `read_text` moved the position; a write starts at the front.
        std::io::Seek::rewind(&mut file).map_err(|e| fault(&e))?;
        file.write_all(content.as_bytes()).map_err(|e| fault(&e))?;
        Ok(before)
    }

    fn remove(&mut self, real: &AbsPath, within: &AbsPath) -> Result<(), FileFault> {
        let file = File::open(real.as_str()).map_err(|e| fault(&e))?;
        still_inside(&file, within)?;
        std::fs::remove_file(real.as_str()).map_err(|e| fault(&e))
    }
}
