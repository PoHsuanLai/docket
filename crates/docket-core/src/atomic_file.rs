//! One durable way to replace a file, and one way to read a file that may not exist yet.
//!
//! A grant file holds the person's consent, so a crash or a failed write must leave the old
//! file or the new one, never half of one and never nothing. [`write_atomic`] writes a
//! temporary file beside the target, syncs it, renames it into place and syncs the directory so
//! the rename itself survives a power cut.

use std::io::{ErrorKind, Write};
use std::path::{Path, PathBuf};

/// The temporary file beside `path`: its name and `.tmp`.
fn temporary_beside(path: &Path) -> std::io::Result<PathBuf> {
    let mut name = path
        .file_name()
        .ok_or_else(|| std::io::Error::new(ErrorKind::InvalidInput, "path has no file name"))?
        .to_owned();
    name.push(".tmp");
    Ok(path.with_file_name(name))
}

fn write_synced(temporary: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut file = std::fs::File::create(temporary)?;
    file.write_all(bytes)?;
    file.sync_all()
}

fn sync_directory(dir: &Path) -> std::io::Result<()> {
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    std::fs::File::open(dir)?.sync_all()
}

/// Replaces the file at `path` with `bytes`, creating its directory when needed. When this
/// returns an error the file still holds what it held before, and no temporary file is left.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = temporary_beside(path)?;
    let dir = path.parent().unwrap_or(Path::new(""));
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(dir)?;
    }
    let placed = write_synced(&temporary, bytes).and_then(|()| std::fs::rename(&temporary, path));
    if let Err(why) = placed {
        // Best effort: the error that matters is the one being returned.
        let _ = std::fs::remove_file(&temporary);
        return Err(why);
    }
    sync_directory(dir)
}

/// The text of the file at `path`, or `None` when there is no such file. Any other failure is
/// an error: an unreadable file is not an empty one.
pub fn read_optional(path: &Path) -> std::io::Result<Option<String>> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(why) if why.kind() == ErrorKind::NotFound => Ok(None),
        Err(why) => Err(why),
    }
}
