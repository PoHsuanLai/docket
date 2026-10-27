//! The local zone, read from the system (`date +%z`) at most once an hour. This is the one place
//! that touches the system for it; everything below is handed a [`Zone`]. Reading the
//! text and how long a reading is trusted are `companion_core`'s.

use companion_core::{ZoneReading, offset_of};
use docket_core::routine::Zone;
use prov::UnixSeconds;
use std::process::Command;
use std::sync::Mutex;

/// The system's zone, remembered for an hour.
#[derive(Debug, Default)]
pub struct SystemZone {
    last: Mutex<ZoneReading>,
}

impl SystemZone {
    /// The zone at `at`: UTC when the system will not say.
    pub fn at(&self, at: UnixSeconds) -> Zone {
        let mut last = match self.last.lock() {
            Ok(last) => last,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some(zone) = last.fresh(at) {
            return zone;
        }
        let zone = Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|out| offset_of(&String::from_utf8_lossy(&out.stdout)))
            .map_or(Zone::UTC, Zone);
        last.keep(at, zone);
        zone
    }
}
