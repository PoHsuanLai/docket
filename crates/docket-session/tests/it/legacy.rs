//! Records companiond wrote before this crate: no version, `companion_wire::SessionRecord`'s form.

use crate::support::*;
use companion_wire::{AnswerPhase, SessionRecord};
use docket_core::SkillId;
use docket_session::*;

fn body(record: &SessionRecord) -> (String, String) {
    (
        record.slug().to_owned(),
        serde_json::to_string(record).expect("json"),
    )
}

fn old_log() -> Vec<Logged> {
    let records = [
        SessionRecord::Opened {
            task: task("t-1"),
            space: space(),
            agent: prov::AgentRef::Companion,
            parent: Some(task("t-0")),
        },
        SessionRecord::Asked {
            turn: turn(1, "archive the newsletters"),
            to: prov::AgentRef::Companion,
            task: task("t-1"),
        },
        SessionRecord::SkillLoaded {
            task: task("t-1"),
            id: SkillId::parse("triage").expect("id"),
            version: docket_core::SkillVersion("0.1.0".into()),
        },
        SessionRecord::Replied {
            task: task("t-1"),
            phase: AnswerPhase::Done,
            digest: almanac_skeleton(),
        },
        SessionRecord::Finished {
            task: task("t-1"),
            phase: AnswerPhase::Done,
        },
        SessionRecord::Closed,
    ];
    records
        .iter()
        .enumerate()
        .map(|(i, r)| {
            let (slug, json) = body(r);
            decode(&slug, &json, Seq(i as u64))
        })
        .collect()
}

fn almanac_skeleton() -> almanac_core::Skeleton {
    docket_core::skeleton_of(&docket_core::TaskLedger {
        task: task("t-1"),
        agent: prov::AgentRef::Companion,
        parent: None,
        space: space(),
        started: prov::UnixSeconds(0),
        asked: vec![],
        steps: vec![],
        touched: vec![],
        results: vec![],
    })
}

#[test]
fn old_records_become_entries_or_typed_legacy() {
    let rows = old_log();
    let kinds: Vec<&str> = rows
        .iter()
        .map(|r| match &r.read {
            Read::Entry(e) => e.slug(),
            Read::Legacy(_) => "legacy",
            Read::Unreadable(_) => "unreadable",
        })
        .collect();
    assert_eq!(
        kinds,
        ["opened", "turn", "skill", "legacy", "legacy", "closed"]
    );
    let Read::Entry(first) = &rows[0].read else {
        panic!("opened");
    };
    let SessionEntry::Opened(opening) = first.as_ref() else {
        panic!("opened");
    };
    assert_eq!(opening.opener, None, "the old record never said");
    assert_eq!(opening.backend, BackendKind::Native);
    assert_eq!(opening.parent, Some(task("t-0")));
}

#[test]
fn an_old_log_resumes_and_counts_what_it_skipped() {
    let plan = resume_plan(&old_log()).expect("plan");
    assert_eq!(plan.standing, Standing::Closed(EndCause::Closed));
    assert_eq!(plan.turns.len(), 1);
    assert_eq!(plan.skills.len(), 1);
    assert_eq!(plan.legacy_skipped.0, 2);
    assert_eq!(plan.taint, Taint::Clean);
    assert!(plan.faults.is_empty());
}

#[test]
fn old_and_new_entries_share_one_log() {
    let mut rows = old_log();
    rows.truncate(2);
    let next = encode(Seq(2), &call(1, "mail.thread.search")).expect("encode");
    rows.push(decode("call", &next.json, Seq(2)));
    let plan = resume_plan(&rows).expect("plan");
    assert_eq!(plan.interrupted.len(), 1);
    assert_eq!(plan.standing, Standing::Open);
}

#[test]
fn a_garbled_old_record_is_unreadable() {
    let got = decode("asked", r#"{"kind":"asked","v":{"nope":1}}"#, Seq(3));
    assert_eq!(got.read, Read::Unreadable(Unreadable::Malformed));
    let got = decode("opened", r#"{"kind":"closed"}"#, Seq(3));
    assert_eq!(got.read, Read::Unreadable(Unreadable::KindMismatch));
}

#[test]
fn the_kind_prefix_is_the_one_companiond_writes() {
    assert_eq!(
        kind_tag("turn"),
        format!("{}.turn", companion_wire::SESSION_KIND_PREFIX)
    );
}
