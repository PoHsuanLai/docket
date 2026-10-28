//! The local zone, read from the system through `chrono::Local`. This is the one place that
//! touches the system for it; everything below is handed a [`Zone`].

use chrono::{DateTime, Local, TimeZone};
use companion_model::routine::Zone;
use prov::UnixSeconds;

/// The system's zone.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemZone;

impl SystemZone {
    /// The zone at `at`, daylight saving included: UTC when `at` is beyond the calendar.
    pub fn at(&self, at: UnixSeconds) -> Zone {
        DateTime::from_timestamp(at.0, 0).map_or(Zone::UTC, |utc| {
            Zone(
                Local
                    .offset_from_utc_datetime(&utc.naive_utc())
                    .local_minus_utc(),
            )
        })
    }
}
