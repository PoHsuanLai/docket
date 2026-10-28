//! The systemd user timers companiond writes for the routines that run at a time. This is their
//! text; putting it on disk is companiond's (`UnitDir`). A timer only wakes the assistant at
//! the routine's time: it asks systemd to start companiond, which keeps the routines and decides
//! what to run. `Persistent=false`, so a time that passes while the computer is off or asleep is
//! not made up later by systemd; companiond sees it was late and skips it (unless the person chose
//! to run it once on waking).

use companion_model::routine::TimerEntry;

/// The name every routine unit starts with.
pub const UNIT_PREFIX: &str = "quire-routine-";

/// One unit file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnitFile {
    /// Its file name, with the extension.
    pub name: String,
    /// Its text.
    pub text: String,
}

fn stem(entry: &TimerEntry) -> String {
    format!("{UNIT_PREFIX}{}", entry.routine.0)
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
