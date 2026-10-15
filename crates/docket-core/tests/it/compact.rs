//! What a summary may say: not empty, not long, and handles only the planner held.

use docket_core::{Handle, SUMMARY_MAX_CHARS, SummaryFault, SummaryText};

#[test]
fn a_summary_cites_only_handles_the_planner_held() {
    let held = [Handle(1), Handle(12)];
    let long = "x".repeat(SUMMARY_MAX_CHARS + 1);
    let table: [(&str, Result<&str, SummaryFault>); 8] = [
        (
            "searched, found #1 and #12",
            Ok("searched, found #1 and #12"),
        ),
        ("  padded  ", Ok("padded")),
        ("no handles at all", Ok("no handles at all")),
        ("a # alone is not a handle", Ok("a # alone is not a handle")),
        ("   ", Err(SummaryFault::Empty)),
        (long.as_str(), Err(SummaryFault::TooLong)),
        ("found #2", Err(SummaryFault::UnknownHandle(2))),
        (
            "found #99999999999999999999999",
            Err(SummaryFault::UnknownHandle(u64::MAX)),
        ),
    ];
    for (words, want) in table {
        let got = SummaryText::parse(words, &held);
        assert_eq!(
            got.as_ref().map(SummaryText::as_str).map_err(|e| *e),
            want,
            "{words}"
        );
    }
}
