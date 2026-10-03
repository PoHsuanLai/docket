//! Dates, instants, durations and decimals as a terminal writes them. A date or an instant is
//! RFC 3339 (`2026-10-03`, `2026-10-03T09:30:00+02:00`). The launcher's natural-date parser
//! ("tomorrow at nine") is sill's and cannot be reached from here; that is an interface ask,
//! and until it lands a natural date is a usage error that says so.

use docket_core::{CivilDate, Decimal, Scale, Seconds};
use prov::UnixSeconds;

/// Why a word is not the value wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum WhenFault {
    /// Not RFC 3339.
    #[error(
        "expected RFC 3339 (2026-10-03 or 2026-10-03T09:30:00Z); natural dates are not understood here"
    )]
    NotRfc3339,
    /// Not a duration.
    #[error("expected seconds (90) or units (1h30m, 2d, 45s)")]
    NotDuration,
    /// Not a decimal with this many digits.
    #[error("expected a decimal number with at most {0} digits after the point")]
    NotDecimal(u8),
}

fn digits(text: &str, len: usize) -> Option<i64> {
    (text.len() == len && text.bytes().all(|b| b.is_ascii_digit()))
        .then(|| text.parse().ok())
        .flatten()
}

fn leap(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in(year: i64, month: i64) -> i64 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn civil(text: &str) -> Option<(i64, i64, i64)> {
    let (y, rest) = text.split_once('-')?;
    let (m, d) = rest.split_once('-')?;
    let (year, month, day) = (digits(y, 4)?, digits(m, 2)?, digits(d, 2)?);
    ((1..=12).contains(&month) && (1..=days_in(year, month)).contains(&day))
        .then_some((year, month, day))
}

/// Days from 1970-01-01 to a civil date (Hinnant's algorithm).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The part of an RFC 3339 instant after the date: `T09:30:00.5+02:00`, as seconds from UTC
/// midnight of that date.
fn clock(text: &str) -> Option<i64> {
    let text = text.strip_prefix(['T', 't', ' '])?;
    let split = text.find(['Z', 'z', '+', '-']).unwrap_or(text.len());
    let (time, zone) = text.split_at(split);
    let mut parts = time.split(':');
    let (h, m, s) = (parts.next()?, parts.next()?, parts.next()?);
    let s = s.split_once('.').map_or(s, |(whole, frac)| {
        if frac.bytes().all(|b| b.is_ascii_digit()) && !frac.is_empty() {
            whole
        } else {
            ""
        }
    });
    let (h, m, s) = (digits(h, 2)?, digits(m, 2)?, digits(s, 2)?);
    if parts.next().is_some() || h > 23 || m > 59 || s > 60 {
        return None;
    }
    let offset = match zone {
        "Z" | "z" => 0,
        zone => {
            let sign = if zone.starts_with('-') { -1 } else { 1 };
            let (oh, om) = zone.get(1..)?.split_once(':')?;
            let (oh, om) = (digits(oh, 2)?, digits(om, 2)?);
            if oh > 23 || om > 59 {
                return None;
            }
            sign * (oh * 3600 + om * 60)
        }
    };
    Some(h * 3600 + m * 60 + s - offset)
}

/// A calendar date: `2026-10-03`, or the date part of an RFC 3339 instant as written.
pub fn date(text: &str) -> Result<CivilDate, WhenFault> {
    let head = text.get(..10).ok_or(WhenFault::NotRfc3339)?;
    let (year, month, day) = civil(head).ok_or(WhenFault::NotRfc3339)?;
    let tail = &text[10..];
    if !tail.is_empty() && clock(tail).is_none() {
        return Err(WhenFault::NotRfc3339);
    }
    Ok(CivilDate {
        year: i16::try_from(year).map_err(|_| WhenFault::NotRfc3339)?,
        month: u8::try_from(month).map_err(|_| WhenFault::NotRfc3339)?,
        day: u8::try_from(day).map_err(|_| WhenFault::NotRfc3339)?,
    })
}

/// An instant: RFC 3339, or a bare date taken as midnight UTC.
pub fn instant(text: &str) -> Result<UnixSeconds, WhenFault> {
    let head = text.get(..10).ok_or(WhenFault::NotRfc3339)?;
    let (year, month, day) = civil(head).ok_or(WhenFault::NotRfc3339)?;
    let tail = &text[10..];
    let seconds = if tail.is_empty() {
        0
    } else {
        clock(tail).ok_or(WhenFault::NotRfc3339)?
    };
    Ok(UnixSeconds(
        days_from_civil(year, month, day) * 86_400 + seconds,
    ))
}

/// A length of time: whole seconds, or `d`, `h`, `m` and `s` units (`1h30m`).
pub fn duration(text: &str) -> Result<Seconds, WhenFault> {
    let mut total: u64 = 0;
    let mut number = String::new();
    let mut saw_unit = false;
    for c in text.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        let unit = match c {
            'd' => 86_400,
            'h' => 3_600,
            'm' => 60,
            's' => 1,
            _ => return Err(WhenFault::NotDuration),
        };
        let n: u64 = number.parse().map_err(|_| WhenFault::NotDuration)?;
        total = total.saturating_add(n.saturating_mul(unit));
        number.clear();
        saw_unit = true;
    }
    if !number.is_empty() {
        if saw_unit {
            return Err(WhenFault::NotDuration);
        }
        total = number.parse().map_err(|_| WhenFault::NotDuration)?;
    }
    if text.is_empty() {
        return Err(WhenFault::NotDuration);
    }
    u32::try_from(total)
        .map(Seconds)
        .map_err(|_| WhenFault::NotDuration)
}

/// A fixed-point number with exactly `scale` digits after the point (missing ones are zeros).
pub fn decimal(text: &str, scale: Scale) -> Result<Decimal, WhenFault> {
    let fault = WhenFault::NotDecimal(scale.0);
    let (negative, body) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text),
    };
    let (whole, frac) = body.split_once('.').unwrap_or((body, ""));
    let plain = |s: &str| s.bytes().all(|b| b.is_ascii_digit());
    if whole.is_empty() || !plain(whole) || !plain(frac) || frac.len() > usize::from(scale.0) {
        return Err(fault);
    }
    let padded = format!("{whole}{frac:0<width$}", width = usize::from(scale.0));
    let units: i64 = padded.parse().map_err(|_| fault)?;
    Ok(Decimal {
        units: if negative { -units } else { units },
        scale,
    })
}
