//! What the host is told when the book changes: the systemd timers wanted (the whole set, only
//! when it differs from the last) and whether the desktop must stay awake (only while a run is in
//! progress).

use super::units::{UnitFile, units_of};
use companion_model::routine::{RoutineBook, TimerEntry};

/// What changed since the host was last told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settled {
    /// The units to install, the whole set, when the timers wanted changed.
    pub units: Option<Vec<UnitFile>>,
    /// Whether the desktop must stay awake (a run is in progress), when that changed. A time is
    /// never waited for awake.
    pub awake: Option<bool>,
}

/// What the host was last told.
#[derive(Debug, Default)]
pub struct Settle {
    /// The timers last installed; none before the first time.
    installed: Option<Vec<TimerEntry>>,
    awake: bool,
}

impl Settle {
    /// What changed in `book` since the host was last told, and now it has been.
    pub fn settle(&mut self, book: &RoutineBook) -> Settled {
        let timers = book.timers();
        let units = (self.installed.as_ref() != Some(&timers)).then(|| units_of(&timers));
        if units.is_some() {
            self.installed = Some(timers);
        }
        let need = book.running() != 0;
        let awake = (self.awake != need).then_some(need);
        self.awake = need;
        Settled { units, awake }
    }
}
