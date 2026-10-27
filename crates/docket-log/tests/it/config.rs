use docket_log::{Config, Sink};
use std::ffi::OsStr;
use tracing::Level;

#[test]
fn the_journal_is_the_sink_only_when_it_listens() {
    let rows = [
        (Some(OsStr::new("8:12345")), Sink::Journal),
        (None, Sink::Stderr),
    ];
    for (stream, want) in rows {
        assert_eq!(Config::from_vars("voiced", stream, None).sink, want);
    }
}

#[test]
fn the_default_keeps_info_and_above() {
    let filter = Config::from_vars("voiced", None, None).filter;
    assert!(filter.would_enable("any", &Level::INFO));
    assert!(filter.would_enable("any", &Level::ERROR));
    assert!(!filter.would_enable("any", &Level::DEBUG));
}

#[test]
fn a_rust_log_filter_picks_levels_by_target() {
    let filter = Config::from_vars("voiced", None, Some("warn,docket_core=debug")).filter;
    assert!(filter.would_enable("docket_core", &Level::DEBUG));
    assert!(!filter.would_enable("other", &Level::INFO));
    assert!(filter.would_enable("other", &Level::WARN));
}

#[test]
fn a_filter_that_does_not_parse_leaves_the_default() {
    let filter = Config::from_vars("voiced", None, Some("info=loud=x")).filter;
    assert!(filter.would_enable("any", &Level::INFO));
    assert!(!filter.would_enable("any", &Level::DEBUG));
}
