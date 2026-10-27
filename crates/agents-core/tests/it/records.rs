//! The install record and the offered record as text.

use agents_core::launch::Launch;
use agents_core::offered::{Named, Offered, Start};
use agents_core::record::{Check, Record, RecordFault};
use std::collections::BTreeMap;
use std::path::Path;

fn record(command: &str) -> Record {
    Record {
        id: "agy-acp".into(),
        version: "1.0.0".into(),
        command: command.into(),
        args: vec!["--uid=".into()],
        env: BTreeMap::from([("K".into(), "v".into())]),
        digest: None,
        check: Check::FirstUse,
    }
}

fn offered(start: Start, models: &[&str]) -> Offered {
    Offered {
        seen: 0,
        start,
        in_use: models.first().map(|m| (*m).to_owned()),
        models: models
            .iter()
            .map(|m| Named {
                id: (*m).to_owned(),
                name: (*m).to_owned(),
            })
            .collect(),
        ways: Vec::new(),
    }
}

#[test]
fn a_record_survives_its_text() {
    let one = record("./agy");
    let text = one.render().expect("render");
    assert_eq!(Record::parse(&text), Ok(one));
}

#[test]
fn a_record_that_climbs_out_or_is_not_one_is_damaged() {
    for bad in [record("../agy"), record("/bin/sh")] {
        let text = bad.render().expect("render");
        assert_eq!(Record::parse(&text), Err(RecordFault::Damaged));
    }
    assert_eq!(Record::parse("nonsense ="), Err(RecordFault::Damaged));
}

#[test]
fn the_launch_is_the_record_at_its_directory() {
    let dir = Path::new("/r/installed/agy-acp/1.0.0");
    let launch = Launch::of(&record("./agy"), dir, "/r/state/agy-acp/home".into());
    assert_eq!(launch.command, dir.join("files/./agy"));
    assert_eq!(launch.files, dir.join("files"));
    assert_eq!(launch.args, ["--uid="]);
    assert_eq!(launch.env, [("K".to_owned(), "v".to_owned())]);
}

#[test]
fn the_offered_counter_counts_writes_and_a_bare_start_keeps_the_models() {
    let first = offered(Start::SignedIn, &["sonnet"]).after(None);
    assert_eq!(first.seen, 1);
    let bare = offered(Start::NeedsSignIn, &[]).after(Some(&first));
    assert_eq!((bare.seen, bare.models.len()), (2, 1));
    assert_eq!(bare.in_use.as_deref(), Some("sonnet"));
    let signed_in = offered(Start::SignedIn, &[]).after(Some(&bare));
    assert!(signed_in.models.is_empty());
    let text = signed_in.render().expect("render");
    assert_eq!(Offered::parse(&text), Ok(signed_in));
}
