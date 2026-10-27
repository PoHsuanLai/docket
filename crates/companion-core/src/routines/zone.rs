//! The local zone as the system reports it (`date +%z`): reading that text, and how long a reading
//! is trusted. Asking the system is companiond's.

use docket_core::routine::Zone;
use prov::UnixSeconds;

/// How long a reading is trusted, in seconds.
const FRESH: i64 = 3_600;

/// The offset in `+0800` or `-0530` form, in seconds east of UTC.
pub fn offset_of(text: &str) -> Option<i32> {
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

/// The last reading of the zone and when it was taken.
#[derive(Debug, Default)]
pub struct ZoneReading {
    last: Option<(UnixSeconds, Zone)>,
}

impl ZoneReading {
    /// The zone read within the hour before `at`, if there is one.
    pub fn fresh(&self, at: UnixSeconds) -> Option<Zone> {
        self.last
            .filter(|(read, _)| (0..FRESH).contains(&(at.0 - read.0)))
            .map(|(_, zone)| zone)
    }

    /// Keeps `zone`, read at `at`.
    pub fn keep(&mut self, at: UnixSeconds, zone: Zone) {
        self.last = Some((at, zone));
    }
}
