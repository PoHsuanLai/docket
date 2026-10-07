//! RFC 3339 dates and instants, as the terminal and the reader both read them.

use docket_core::CivilDate;
use docket_core::when::{NotRfc3339, date, instant};
use prov::UnixSeconds;

#[test]
fn dates_table() {
    let rows: [(&str, Result<CivilDate, NotRfc3339>); 7] = [
        (
            "2026-10-03",
            Ok(CivilDate {
                year: 2026,
                month: 10,
                day: 3,
            }),
        ),
        (
            "2026-10-03T09:30:00Z",
            Ok(CivilDate {
                year: 2026,
                month: 10,
                day: 3,
            }),
        ),
        (
            "2024-02-29",
            Ok(CivilDate {
                year: 2024,
                month: 2,
                day: 29,
            }),
        ),
        ("2026-02-29", Err(NotRfc3339)),
        ("tomorrow", Err(NotRfc3339)),
        ("2026-13-01", Err(NotRfc3339)),
        ("2026-10-03Tnine", Err(NotRfc3339)),
    ];
    for (text, want) in rows {
        assert_eq!(date(text), want, "{text}");
    }
}

#[test]
fn instants_table() {
    // 2026-10-03 is 20 729 days after the epoch.
    let rows: [(&str, Result<UnixSeconds, NotRfc3339>); 6] = [
        ("1970-01-01", Ok(UnixSeconds(0))),
        ("1970-01-02T00:00:00Z", Ok(UnixSeconds(86_400))),
        ("2026-10-03T09:30:00+02:00", Ok(UnixSeconds(1_791_012_600))),
        ("2026-10-03T09:30:00.5Z", Ok(UnixSeconds(1_791_019_800))),
        ("2026-10-03T25:00:00Z", Err(NotRfc3339)),
        ("soon", Err(NotRfc3339)),
    ];
    for (text, want) in rows {
        assert_eq!(instant(text), want, "{text}");
    }
}
