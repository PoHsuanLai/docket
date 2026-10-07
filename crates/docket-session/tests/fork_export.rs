mod support;

use docket_core::BreakerTrip;
use docket_session::*;
use support::*;

fn parent_rows() -> Vec<Logged> {
    rows(&canonical())
}

#[test]
fn a_fork_inherits_taint_and_the_breaker_and_resets_budgets() {
    // Cut after the trip and before the turn that would clear it.
    let child = fork(&session("s-1"), &parent_rows(), Seq(8), task("t-2")).expect("fork");
    let plan = resume_plan(&rows(&child)).expect("plan");
    assert_eq!(plan.taint, Taint::Tainted);
    assert_eq!(plan.standing, Standing::Paused(BreakerTrip::Consecutive));
    assert_eq!(plan.budget.checkpoint, None);
    assert_eq!(plan.budget.calls_since.0, 2);
    assert_eq!(plan.policy, Some(policy()));
    let SessionEntry::Opened(opening) = &child[0] else {
        panic!("first entry is the opening");
    };
    assert_eq!(opening.task, task("t-2"));
    assert_eq!(
        opening.forked_from,
        Some(ForkPoint {
            session: session("s-1"),
            at: Seq(8)
        })
    );
    assert!(matches!(&child[1], SessionEntry::Taint(t) if t.cause == TaintCause::Inherited));
}

#[test]
fn a_fork_of_a_closed_session_is_open_and_has_no_budget_entry() {
    let child = fork(&session("s-1"), &parent_rows(), Seq(13), task("t-2")).expect("fork");
    assert!(
        child
            .iter()
            .all(|e| !matches!(e, SessionEntry::Closed(_) | SessionEntry::Budget(_)))
    );
    let plan = resume_plan(&rows(&child)).expect("plan");
    assert_eq!(plan.standing, Standing::Open);
    assert_eq!(plan.history.len(), 2);
}

#[test]
fn a_clean_parent_gives_a_clean_child() {
    let child = fork(&session("s-1"), &parent_rows(), Seq(2), task("t-2")).expect("fork");
    let plan = resume_plan(&rows(&child)).expect("plan");
    assert_eq!(plan.taint, Taint::Clean);
    assert_eq!(child.len(), 3);
}

#[test]
fn a_fork_of_a_tainted_by_fault_parent_writes_the_taint_down() {
    let broken = rows(&[SessionEntry::Opened(opening()), untrusted_handle(1)]);
    let child = fork(&session("s-1"), &broken, Seq(1), task("t-2")).expect("fork");
    let plan = resume_plan(&rows(&child)).expect("plan");
    assert_eq!(plan.taint, Taint::Tainted);
    assert!(
        plan.faults.is_empty(),
        "the child's log is in write-ahead order"
    );
}

#[test]
fn a_fork_refuses_what_it_cannot_copy_whole() {
    assert_eq!(
        fork(&session("s-1"), &parent_rows(), Seq(99), task("t-2")),
        Err(ForkFault::PastEnd)
    );
    let mut gappy = parent_rows();
    gappy[3].seq = Seq(30);
    gappy.truncate(5);
    assert_eq!(
        fork(&session("s-1"), &gappy, Seq(4), task("t-2")),
        Err(ForkFault::NotWhole)
    );
    assert_eq!(
        fork(&session("s-1"), &[], Seq(0), task("t-2")),
        Err(ForkFault::PastEnd)
    );
}

#[test]
fn an_export_round_trips_and_is_stable() {
    let doc = export(&session("s-1"), &parent_rows()).expect("export");
    let json = to_json(&doc).expect("json");
    assert_eq!(to_json(&doc).expect("json"), json);
    let back = from_json(&json).expect("import");
    assert_eq!(back, doc);
    assert_eq!(
        resume_plan(&rows(&back.entries)),
        resume_plan(&parent_rows())
    );
}

#[test]
fn an_export_names_labels_not_values_and_refuses_a_hole() {
    let json = to_json(&export(&session("s-1"), &parent_rows()).expect("export")).expect("json");
    assert!(json.contains("\"handle\""), "handle labels are there");
    assert!(
        !json.contains("\"text\":\"mail"),
        "no handle content exists to write"
    );
    let mut bad = parent_rows();
    bad[2].read = Read::Unreadable(Unreadable::Malformed);
    assert_eq!(
        export(&session("s-1"), &bad),
        Err(ExportFault::Unreadable(Seq(2)))
    );
}

#[test]
fn an_import_checks_its_version() {
    assert_eq!(from_json("{"), Err(ExportFault::Malformed));
    assert_eq!(
        from_json(r#"{"export_version":9,"anything":1}"#),
        Err(ExportFault::UnknownVersion(9))
    );
}
