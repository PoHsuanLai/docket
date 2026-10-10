use crate::support::*;
use docket_session::*;

#[test]
fn every_entry_round_trips_with_its_kind_tag_and_version() {
    for (i, entry) in canonical().iter().enumerate() {
        let enc = encode(Seq(i as u64), entry).expect("encode");
        assert_eq!(enc.kind, format!("companion.session.{}", entry.slug()));
        assert!(enc.json.contains("\"version\":1"));
        let got = decode(entry.slug(), &enc.json, Seq(99));
        assert_eq!(got.seq, Seq(i as u64), "the stored seq wins over the place");
        assert_eq!(got.read, Read::Entry(Box::new(entry.clone())));
    }
}

#[test]
fn an_unknown_version_is_unreadable_not_guessed() {
    let got = decode(
        "turn",
        r#"{"version":2,"seq":4,"kind":"turn","v":{}}"#,
        Seq(0),
    );
    assert_eq!(
        got.read,
        Read::Unreadable(Unreadable::UnknownVersion(EntryVersion(2)))
    );
    let got = decode("turn", r#"{"version":"one","kind":"turn"}"#, Seq(0));
    assert_eq!(got.read, Read::Unreadable(Unreadable::BadVersion));
}

#[test]
fn bodies_that_do_not_fit_say_how() {
    let cases = [
        ("turn", "nope", Unreadable::NotJson),
        ("turn", "[1]", Unreadable::NotJson),
        (
            "turn",
            r#"{"version":1,"kind":"future","v":1}"#,
            Unreadable::UnknownKind,
        ),
        (
            "turn",
            r#"{"version":1,"kind":"turn","v":{"id":"x"}}"#,
            Unreadable::Malformed,
        ),
        (
            "turn",
            r#"{"version":1,"kind":"closed","v":"closed"}"#,
            Unreadable::KindMismatch,
        ),
    ];
    for (slug, json, want) in cases {
        assert_eq!(
            decode(slug, json, Seq(0)).read,
            Read::Unreadable(want),
            "{json}"
        );
    }
}

#[test]
fn a_session_log_page_reads_back_what_was_appended() {
    use docket_session::fake::{LogMood, MemoryLog};
    let log = MemoryLog::new();
    let s = session("s-1");
    for (i, e) in canonical().iter().enumerate() {
        let ack = block_on(log.append(&s, Seq(i as u64), e)).expect("append");
        assert_eq!(ack.seq, Seq(i as u64));
    }
    let size = PageSize(porter_core::Count(5));
    let first = block_on(log.page(&s, None, size)).expect("page");
    assert_eq!(first.rows.len(), 5);
    assert_eq!(first.next, Some(Seq(5)));
    let mut all = first.rows;
    let mut next = first.next;
    while let Some(from) = next {
        let page = block_on(log.page(&s, Some(from), size)).expect("page");
        all.extend(page.rows);
        next = page.next;
    }
    assert_eq!(all, rows(&canonical()));

    // Out of order is refused; a refusing or absent store fails the append.
    assert_eq!(
        block_on(log.append(&s, Seq(3), &canonical()[0])),
        Err(LogFault::OutOfOrder { expected: Seq(14) })
    );
    log.set_mood(LogMood::Refusing);
    assert_eq!(
        block_on(log.append(&s, Seq(14), &taint())),
        Err(LogFault::Refused)
    );
    log.set_mood(LogMood::Down);
    assert_eq!(
        block_on(log.append(&s, Seq(14), &taint())),
        Err(LogFault::Unavailable)
    );
}

#[test]
fn an_unknown_version_in_the_log_blocks_the_resume() {
    use docket_session::fake::MemoryLog;
    let log = MemoryLog::new();
    let s = session("s-1");
    block_on(log.append(&s, Seq(0), &SessionEntry::Opened(opening()))).expect("append");
    log.put_raw(
        &s,
        "companion.session.taint",
        r#"{"version":7,"kind":"taint","v":{}}"#,
    );
    let page = block_on(log.page(&s, None, PageSize(porter_core::Count(10)))).expect("page");
    let plan = resume_plan(&page.rows).expect("plan");
    assert_eq!(plan.standing, Standing::Blocked(Blocker::Unreadable));
    assert_eq!(plan.taint, Taint::Tainted);
}

#[test]
fn an_opening_written_before_started_from_reads_back_with_none() {
    let scope = docket_core::TerminalScope::from_cgroup("vte-spawn-1.scope").expect("scope");
    let with = Opening {
        started_from: Some(docket_core::StartedFrom::Terminal(scope)),
        ..opening()
    };
    assert!(
        serde_json::to_string(&with)
            .expect("json")
            .contains("vte-spawn-1.scope")
    );
    // What a log wrote before the field existed: the same object without the key.
    let mut old = serde_json::to_value(opening()).expect("value");
    assert!(
        old.as_object_mut()
            .expect("object")
            .remove("started_from")
            .is_none()
    );
    let read: Opening = serde_json::from_value(old).expect("an old opening reads");
    assert_eq!(read.started_from, None);
}

#[test]
fn an_opening_written_before_the_label_reads_back_with_none_and_a_label_round_trips() {
    let with = Opening {
        label: Some(prov::AgentLabel("Claude Code".to_owned())),
        ..opening()
    };
    let text = serde_json::to_string(&with).expect("json");
    assert!(text.contains("Claude Code"));
    let back: Opening = serde_json::from_str(&text).expect("reads");
    assert_eq!(back, with);
    // No label: the key is not written, and a log without it reads as none.
    let mut old = serde_json::to_value(opening()).expect("value");
    assert!(
        old.as_object_mut()
            .expect("object")
            .remove("label")
            .is_none()
    );
    let read: Opening = serde_json::from_value(old).expect("an old opening reads");
    assert_eq!(read.label, None);
}

#[test]
fn a_watch_opening_round_trips_with_its_rewind_and_an_ordinary_one_is_unchanged() {
    let watch = Opening {
        backend: BackendKind::Watch(docket_core::Rewind::Agent),
        ..opening()
    };
    let text = serde_json::to_string(&watch).expect("json");
    assert!(
        text.contains(r#""backend":{"kind":"watch","v":"agent"}"#),
        "{text}"
    );
    let back: Opening = serde_json::from_str(&text).expect("reads");
    assert_eq!(back, watch);
    // A log written before watch openings existed names the same backends as ever.
    let old = serde_json::to_string(&opening()).expect("json");
    assert!(!old.contains("watch"), "{old}");
}
