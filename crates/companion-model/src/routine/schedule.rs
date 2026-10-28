//! When a routine runs by the clock: a small, closed set, read from plain words ("daily 08:00",
//! "weekdays 08:00", "every 2 hours") and shown in plain words. Nothing here is a cron string; the
//! one systemd calendar form is `Schedule::calendar`, for the timers companiond writes.

use prov::UnixSeconds;
use serde::{Deserialize, Serialize};
use std::fmt;

const DAY: i64 = 86_400;
const HOUR: i64 = 3_600;
/// The most hours between two runs of an hourly schedule.
const MOST_HOURS: u8 = 12;

/// Why some words are not a schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ScheduleFault {
    /// Not a time of day.
    #[error("say a time of day like 08:00")]
    Time,
    /// Not one of the ways to say when.
    #[error("say when like this: daily 08:00, weekdays 08:00 or every 2 hours")]
    Words,
    /// A number of hours that is out of range.
    #[error("say a number of hours from 1 to 12")]
    Hours,
}

/// How far the local clock is from UTC, in seconds east, at the moment asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Zone(pub i32);

impl Zone {
    /// UTC itself.
    pub const UTC: Zone = Zone(0);
}

/// A time of day on the local clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct TimeOfDay {
    hour: u8,
    minute: u8,
}

impl TimeOfDay {
    /// The time `hour`:`minute`, if it is one.
    pub fn new(hour: u8, minute: u8) -> Result<Self, ScheduleFault> {
        if hour < 24 && minute < 60 {
            Ok(Self { hour, minute })
        } else {
            Err(ScheduleFault::Time)
        }
    }

    /// The time written `8:00` or `08:00`.
    pub fn parse(text: &str) -> Result<Self, ScheduleFault> {
        let (hour, minute) = text.trim().split_once(':').ok_or(ScheduleFault::Time)?;
        if minute.len() != 2 {
            return Err(ScheduleFault::Time);
        }
        let hour = hour.parse().map_err(|_| ScheduleFault::Time)?;
        let minute = minute.parse().map_err(|_| ScheduleFault::Time)?;
        Self::new(hour, minute)
    }

    fn seconds(self) -> i64 {
        i64::from(self.hour) * HOUR + i64::from(self.minute) * 60
    }

    /// As a person says it: `8:00`.
    pub fn plain(self) -> String {
        format!("{}:{:02}", self.hour, self.minute)
    }
}

impl fmt::Display for TimeOfDay {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}:{:02}", self.hour, self.minute)
    }
}

impl TryFrom<String> for TimeOfDay {
    type Error = ScheduleFault;
    fn try_from(text: String) -> Result<Self, ScheduleFault> {
        Self::parse(&text)
    }
}

impl From<TimeOfDay> for String {
    fn from(time: TimeOfDay) -> String {
        time.to_string()
    }
}

/// Hours between two runs: 1 to 12.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "u8", into = "u8")]
pub struct Hours(u8);

impl Hours {
    /// `n` hours, if that is from 1 to 12.
    pub fn new(n: u8) -> Result<Self, ScheduleFault> {
        if (1..=MOST_HOURS).contains(&n) {
            Ok(Self(n))
        } else {
            Err(ScheduleFault::Hours)
        }
    }
}

impl TryFrom<u8> for Hours {
    type Error = ScheduleFault;
    fn try_from(n: u8) -> Result<Self, ScheduleFault> {
        Self::new(n)
    }
}

impl From<Hours> for u8 {
    fn from(hours: Hours) -> u8 {
        hours.0
    }
}

/// A rule in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Schedule {
    /// Every day at a time.
    Daily(TimeOfDay),
    /// Monday to Friday at a time.
    Weekdays(TimeOfDay),
    /// Every N hours, counted from midnight: every 2 hours is 0:00, 2:00, 4:00 and so on.
    Every(Hours),
}

fn is_weekday(days_since_epoch: i64) -> bool {
    // 1970-01-01 was a Thursday; Monday is 0.
    (days_since_epoch + 3).rem_euclid(7) < 5
}

impl Schedule {
    /// The schedule written in plain words.
    pub fn parse(text: &str) -> Result<Self, ScheduleFault> {
        let lower = text.trim().to_lowercase();
        let words: Vec<&str> = lower.split_whitespace().collect();
        match words.as_slice() {
            ["daily", time] => Ok(Schedule::Daily(TimeOfDay::parse(time)?)),
            ["weekdays", time] => Ok(Schedule::Weekdays(TimeOfDay::parse(time)?)),
            ["every", "hour"] => Ok(Schedule::Every(Hours::new(1)?)),
            ["every", n, "hours"] => {
                let n = n.parse().map_err(|_| ScheduleFault::Hours)?;
                Ok(Schedule::Every(Hours::new(n)?))
            }
            _ => Err(ScheduleFault::Words),
        }
    }

    /// The schedule as a person says it.
    pub fn plain(self) -> String {
        match self {
            Schedule::Daily(time) => format!("Every day at {}", time.plain()),
            Schedule::Weekdays(time) => format!("Weekdays at {}", time.plain()),
            Schedule::Every(Hours(1)) => "Every hour".to_owned(),
            Schedule::Every(Hours(n)) => format!("Every {n} hours"),
        }
    }

    /// The same rule as a systemd `OnCalendar=` value, for the timers companiond writes.
    pub fn calendar(self) -> String {
        match self {
            Schedule::Daily(time) => format!("*-*-* {time}:00"),
            Schedule::Weekdays(time) => format!("Mon..Fri *-*-* {time}:00"),
            Schedule::Every(Hours(n)) => format!("*-*-* 0/{n}:00:00"),
        }
    }

    /// The first time after `now` the rule names, on the clock of `zone`.
    pub fn next_after(self, now: UnixSeconds, zone: Zone) -> UnixSeconds {
        let offset = i64::from(zone.0);
        let local = now.0 + offset;
        let day = local.div_euclid(DAY);
        let next = match self {
            Schedule::Daily(time) => {
                let at = day * DAY + time.seconds();
                if at > local { at } else { at + DAY }
            }
            Schedule::Weekdays(time) => (day..day + 8)
                .map(|d| (d, d * DAY + time.seconds()))
                .find(|(d, at)| *at > local && is_weekday(*d))
                .map_or(local + DAY, |(_, at)| at),
            Schedule::Every(Hours(n)) => {
                let step = i64::from(n) * HOUR;
                let slot = (local.rem_euclid(DAY).div_euclid(step) + 1) * step;
                if slot < DAY {
                    day * DAY + slot
                } else {
                    (day + 1) * DAY
                }
            }
        };
        UnixSeconds(next - offset)
    }
}
