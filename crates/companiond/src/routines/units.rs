//! The systemd user timers companiond writes for the routines that run at a time. The text is
//! pure; putting it on disk is the host's (see [`UnitDir`]). A timer only wakes the assistant at
//! the routine's time: it asks systemd to start companiond, which keeps the routines and decides
//! what to run. `Persistent=false`, so a time that passes while the computer is off or asleep is
//! not made up later by systemd; companiond sees it was late and skips it (unless the person chose
//! to run it once on waking).

use docket_core::routine::TimerEntry;
use docket_core::{read_optional, write_atomic};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The name every routine unit starts with.
const PREFIX: &str = "quire-routine-";

/// One unit file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFile {
    /// Its file name, with the extension.
    pub name: String,
    /// Its text.
    pub text: String,
}

fn stem(entry: &TimerEntry) -> String {
    format!("{PREFIX}{}", entry.routine.0)
}

fn timer(entry: &TimerEntry) -> UnitFile {
    let stem = stem(entry);
    let text = format!(
        "# Written by the assistant for a routine; rewritten whenever the routines change.\n\
         [Unit]\n\
         Description=Quire routine {id}: wake the assistant at its time\n\
         \n\
         [Timer]\n\
         OnCalendar={calendar}\n\
         Persistent=false\n\
         AccuracySec=1s\n\
         Unit={stem}.service\n\
         \n\
         [Install]\n\
         WantedBy=timers.target\n",
        id = entry.routine,
        calendar = entry.schedule.calendar(),
    );
    UnitFile {
        name: format!("{stem}.timer"),
        text,
    }
}

fn service(entry: &TimerEntry) -> UnitFile {
    let text = format!(
        "# Written by the assistant for a routine; rewritten whenever the routines change.\n\
         [Unit]\n\
         Description=Quire routine {id}: start the assistant\n\
         \n\
         [Service]\n\
         Type=oneshot\n\
         ExecStart=/usr/bin/systemctl --user start companiond.service\n",
        id = entry.routine,
    );
    UnitFile {
        name: format!("{}.service", stem(entry)),
        text,
    }
}

/// The units for `entries`: a timer and the service it starts, for each.
pub fn units_of(entries: &[TimerEntry]) -> Vec<UnitFile> {
    entries
        .iter()
        .flat_map(|entry| [timer(entry), service(entry)])
        .collect()
}

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
            if name.starts_with(PREFIX) && !wanted.contains(name.as_str()) {
                std::fs::remove_file(entry.path())?;
                changed.push(name);
            }
        }
        Ok(changed)
    }
}
