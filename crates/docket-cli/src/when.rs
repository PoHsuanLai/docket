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

fn rfc3339<T>(read: Result<T, docket_core::when::NotRfc3339>) -> Result<T, WhenFault> {
    read.map_err(|_| WhenFault::NotRfc3339)
}

/// A calendar date: `2026-10-03`, or the date part of an RFC 3339 instant as written.
pub fn date(text: &str) -> Result<CivilDate, WhenFault> {
    rfc3339(docket_core::when::date(text))
}

/// An instant: RFC 3339, or a bare date taken as midnight UTC.
pub fn instant(text: &str) -> Result<UnixSeconds, WhenFault> {
    rfc3339(docket_core::when::instant(text))
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
