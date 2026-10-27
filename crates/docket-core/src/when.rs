//! RFC 3339 dates and instants: `2026-10-03`, `2026-10-03T09:30:00+02:00`. Pure; the terminal
//! (`quire-do`) and the quarantined reader (a `Date` or `DateTime` answer) both read them, and
//! the edges that print a UTC time or day ask here too. The calendar is `chrono`'s; this file
//! only fixes which texts are accepted.

use crate::value::CivilDate;
use chrono::{DateTime, Datelike, NaiveDate, SecondsFormat, Timelike};
use prov::UnixSeconds;

/// The text is not an RFC 3339 date or instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not an RFC 3339 date or instant")]
pub struct NotRfc3339;

/// Nanoseconds a second holds before a leap second is counted in `chrono`.
const SECOND_NANOS: u32 = 1_000_000_000;

/// The date part, `YYYY-MM-DD` exactly: `chrono`'s own reader would also take a sign or a short
/// month.
fn day(head: &str) -> Result<NaiveDate, NotRfc3339> {
    let shaped = head.bytes().enumerate().all(|(at, b)| match at {
        4 | 7 => b == b'-',
        _ => b.is_ascii_digit(),
    });
    match shaped {
        true => NaiveDate::parse_from_str(head, "%Y-%m-%d").map_err(|_| NotRfc3339),
        false => Err(NotRfc3339),
    }
}

/// The text read as a date alone or as a full instant (the separator may be `T`, `t` or a space,
/// the offset `Z`, `z` or numeric).
enum Read {
    Day(NaiveDate),
    Instant(DateTime<chrono::FixedOffset>),
}

fn read(text: &str) -> Result<Read, NotRfc3339> {
    let head = text.get(..10).ok_or(NotRfc3339)?;
    let date = day(head)?;
    match text.len() {
        10 => Ok(Read::Day(date)),
        _ => DateTime::parse_from_rfc3339(text)
            .map(Read::Instant)
            .map_err(|_| NotRfc3339),
    }
}

/// A calendar date: `2026-10-03`, or the date part of an RFC 3339 instant as written.
pub fn date(text: &str) -> Result<CivilDate, NotRfc3339> {
    let date = match read(text)? {
        Read::Day(date) => date,
        Read::Instant(instant) => instant.date_naive(),
    };
    Ok(CivilDate {
        year: i16::try_from(date.year()).map_err(|_| NotRfc3339)?,
        month: u8::try_from(date.month()).map_err(|_| NotRfc3339)?,
        day: u8::try_from(date.day()).map_err(|_| NotRfc3339)?,
    })
}

/// An instant: RFC 3339, or a bare date taken as midnight UTC. A leap second (`:60`) counts as
/// the second after `:59`.
pub fn instant(text: &str) -> Result<UnixSeconds, NotRfc3339> {
    Ok(UnixSeconds(match read(text)? {
        Read::Day(date) => date.and_time(chrono::NaiveTime::MIN).and_utc().timestamp(),
        Read::Instant(instant) => {
            let leap = i64::from(instant.nanosecond() >= SECOND_NANOS);
            instant.timestamp() + leap
        }
    }))
}

/// `at` as an RFC 3339 UTC time, `2026-10-03T09:30:00Z`; none when it is beyond the calendar.
pub fn utc_instant(at: UnixSeconds) -> Option<String> {
    DateTime::from_timestamp(at.0, 0).map(|t| t.to_rfc3339_opts(SecondsFormat::Secs, true))
}

/// The UTC calendar day of `at`, `2026-10-03`; none when it is beyond the calendar.
pub fn utc_day(at: UnixSeconds) -> Option<String> {
    DateTime::from_timestamp(at.0, 0).map(|t| t.format("%Y-%m-%d").to_string())
}
