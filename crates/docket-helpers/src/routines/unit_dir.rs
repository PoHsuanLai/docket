//! The directory the systemd user timers are written into. Their text is
//! `companion_core::units_of`'s; putting it on disk is this crate's.

use companion_core::{UNIT_PREFIX, UnitFile};
use docket_store::{read_optional, write_atomic};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// A directory the units are written into (`~/.config/systemd/user`).
#[derive(Debug, Clone)]
pub struct UnitDir {
    dir: PathBuf,
}

impl UnitDir {
    /// The directory `dir`.
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// The directory.
    pub fn path(&self) -> &Path {
        &self.dir
    }

    /// Makes the directory hold exactly `units` among the routine units: a file whose text is
    /// already right is left alone, and a routine unit that is not wanted any more is removed.
    /// Returns the names that were written or removed, so the caller can tell systemd.
    pub fn install(&self, units: &[UnitFile]) -> std::io::Result<Vec<String>> {
        let mut changed = Vec::new();
        for unit in units {
            let path = self.dir.join(&unit.name);
            if read_optional(&path)?.as_deref() != Some(unit.text.as_str()) {
                write_atomic(&path, unit.text.as_bytes())?;
                changed.push(unit.name.clone());
            }
        }
        let wanted: BTreeSet<&str> = units.iter().map(|u| u.name.as_str()).collect();
        let listing = match std::fs::read_dir(&self.dir) {
            Ok(listing) => listing,
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => return Ok(changed),
            Err(why) => return Err(why),
        };
        for entry in listing.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(UNIT_PREFIX) && !wanted.contains(name.as_str()) {
                std::fs::remove_file(entry.path())?;
                changed.push(name);
            }
        }
        Ok(changed)
    }
}
