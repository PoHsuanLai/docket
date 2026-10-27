//! One durable way to replace a file, and one way to read a file that may not exist yet.
//!
//! A grant file holds the person's consent, so a crash or a failed write must leave the old
//! file or the new one, never half of one and never nothing. [`write_atomic`] writes through
//! `atomic-write-file`: a temporary file beside the target, synced, renamed into place, with the
//! directory synced so the rename itself survives a power cut.

use atomic_write_file::AtomicWriteFile;
use std::io::{ErrorKind, Write};
use std::path::Path;

/// Replaces the file at `path` with `bytes`, creating its directory when needed. When this
/// returns an error the file still holds what it held before.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(dir) = path.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = AtomicWriteFile::open(path)?;
    file.write_all(bytes)?;
    file.commit()
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
