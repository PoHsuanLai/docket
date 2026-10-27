//! The system's zone: reading its text, and how long a reading is trusted.

use companion_core::{ZoneReading, offset_of};
use docket_core::routine::Zone;
use prov::UnixSeconds;

#[test]
fn the_offset_is_read_in_seconds_east_of_utc() {
    assert_eq!(offset_of("+0800\n"), Some(8 * 3_600));
    assert_eq!(offset_of("-0530"), Some(-(5 * 3_600 + 30 * 60)));
    assert_eq!(offset_of("+0000"), Some(0));
}

#[test]
fn words_that_are_not_an_offset_are_not_read() {
    for text in ["", "0800", "+08", "+08:00", "+08000", "+08a0", "UTC"] {
        assert_eq!(offset_of(text), None, "{text:?}");
    }
}

#[test]
fn a_reading_is_trusted_for_an_hour() {
    let mut reading = ZoneReading::default();
    assert_eq!(reading.fresh(UnixSeconds(100)), None);
    reading.keep(UnixSeconds(100), Zone(3_600));
    assert_eq!(reading.fresh(UnixSeconds(100)), Some(Zone(3_600)));
    assert_eq!(reading.fresh(UnixSeconds(3_699)), Some(Zone(3_600)));
    assert_eq!(reading.fresh(UnixSeconds(3_700)), None);
    // A clock that went back does not trust a reading from its future.
    assert_eq!(reading.fresh(UnixSeconds(99)), None);
}
