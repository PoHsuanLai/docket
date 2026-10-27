//! What the host is told when the book changes: the systemd timers wanted (the whole set, only
//! when it differs from the last) and whether the desktop must stay awake (only while a run is in
//! progress).

use super::units::{UnitFile, units_of};
use docket_core::routine::{RoutineBook, TimerEntry};

/// Whether the desktop must stay awake.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Awake {
    /// A run is in progress: do not suspend.
    Needed,
    /// Nothing is running: the desktop may sleep. A time is never waited for awake.
    #[default]
    NotNeeded,
}

/// What changed since the host was last told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    /// The units to install, the whole set, when the timers wanted changed.
    pub units: Option<Vec<UnitFile>>,
    /// The desktop's need to stay awake, when it changed.
    pub awake: Option<Awake>,
}

/// What the host was last told.
#[derive(Debug, Default)]
pub struct Settle {
    /// The timers last installed; none before the first time.
    installed: Option<Vec<TimerEntry>>,
    awake: Awake,
}

impl Settle {
    /// What changed in `book` since the host was last told, and now it has been.
    pub fn settle(&mut self, book: &RoutineBook) -> Settled {
        let timers = book.timers();
        let units = (self.installed.as_ref() != Some(&timers)).then(|| units_of(&timers));
        if units.is_some() {
            self.installed = Some(timers);
        }
        let need = match book.running() {
            0 => Awake::NotNeeded,
            _ => Awake::Needed,
        };
        let awake = (self.awake != need).then_some(need);
        self.awake = need;
        Settled { units, awake }
    }
}
