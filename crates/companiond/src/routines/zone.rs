//! The local zone, read from the system (`date +%z`) at most once an hour. companiond is one of the
//! few crates that may touch the system; everything below it is handed a [`Zone`].

use docket_core::routine::Zone;
use prov::UnixSeconds;
use std::process::Command;
use std::sync::Mutex;

/// How long a reading is trusted, in seconds.
const FRESH: i64 = 3_600;

/// The offset in `+0800` or `-0530` form, in seconds east of UTC.
pub(super) fn offset_of(text: &str) -> Option<i32> {
    let text = text.trim();
    let (sign, digits) = match text.split_at_checked(1)? {
        ("+", digits) => (1, digits),
        ("-", digits) => (-1, digits),
        _ => return None,
    };
    if digits.len() != 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let hours: i32 = digits[..2].parse().ok()?;
    let minutes: i32 = digits[2..].parse().ok()?;
    Some(sign * (hours * 3_600 + minutes * 60))
}

/// The system's zone, remembered for an hour.
#[derive(Debug, Default)]
pub struct SystemZone {
    last: Mutex<Option<(UnixSeconds, Zone)>>,
}

impl SystemZone {
    /// The zone at `at`: UTC when the system will not say.
    pub fn at(&self, at: UnixSeconds) -> Zone {
        let mut last = match self.last.lock() {
            Ok(last) => last,
            Err(poisoned) => poisoned.into_inner(),
        };
        if let Some((read, zone)) = *last
            && (0..FRESH).contains(&(at.0 - read.0))
        {
            return zone;
        }
        let zone = Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|out| offset_of(&String::from_utf8_lossy(&out.stdout)))
            .map_or(Zone::UTC, Zone);
        *last = Some((at, zone));
        zone
    }
}
