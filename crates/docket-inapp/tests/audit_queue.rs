//! The audit records waiting for memory survive an app restart: a file the app names, bounded,
//! written atomically, typed errors, flushed by the next agent. The memory is almanac-fake's
//! backend behind a switch the test flips; every file is in a scratch directory.

mod support;

use almanac_client::InProcess;
use almanac_core::{BodyMode, Caller, MemoryReply, MemoryRequest, RecentQuery, TrustFilter};
use almanac_fake::{ScriptedConsolidator, fake_service};
use docket_inapp::{
    AlmanacMemory, AuditFile, AuditFileError, AuditTo, InAppAgent, InAppKit, SheetAnswer,
};
use docket_router::{LinkFault, MemoryLink};
use porter_core::Count;
use prov::UnixSeconds;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use support::TestSheet;
use support::host::{ARCHIVE, clock, parts, thread, work};
use support::infer::{ScriptedInfer, call, words};

/// Memory that is away while the switch is off.
#[derive(Debug)]
struct Switched<M> {
    inner: M,
    up: Arc<AtomicBool>,
}

impl<M: MemoryLink> MemoryLink for Switched<M> {
    async fn ask(&self, request: MemoryRequest) -> Result<MemoryReply, LinkFault> {
        if self.up.load(Ordering::SeqCst) {
            self.inner.ask(request).await
        } else {
            Err(LinkFault::Unavailable)
        }
    }
}

macro_rules! memory {
    ($service:expr, $up:expr) => {
        Switched {
            inner: AlmanacMemory::over(InProcess::new($service.clone(), Caller::Router)),
            up: $up.clone(),
        }
    };
}

/// A turn that archives a thread: the router audits the call, the confirmation and more.
fn archive_model() -> ScriptedInfer {
    ScriptedInfer::new(vec![
        call(ARCHIVE, json!({ "target": [thread("t2")] })),
        words("Archived the digest."),
    ])
}

fn sheet() -> TestSheet {
    TestSheet::answering(vec![SheetAnswer::Once; 2])
}

/// How many events of `kind` memory holds for the work Space.
async fn held(reader: &impl MemoryLink, kind: &str) -> usize {
    let reply = reader
        .ask(MemoryRequest::Recent(
            work(),
            RecentQuery {
                since: UnixSeconds(0),
                kinds: Vec::new(),
                trust: TrustFilter::Any,
                limit: Count(500),
                bodies: BodyMode::Without,
            },
        ))
        .await
        .expect("recent");
    let MemoryReply::Recent(entries) = reply else {
        panic!("recent: {reply:?}");
    };
    entries
        .iter()
        .filter(|e| format!("{:?}", e.summary.kind).contains(kind))
        .count()
}

#[tokio::test]
async fn records_memory_could_not_take_are_written_by_the_next_agent_after_a_restart() {
    let service = Arc::new(fake_service(ScriptedConsolidator::default()));
    let dir = tempfile::tempdir().expect("scratch");
    let file = dir.path().join("audit-queue.json");
    let up = Arc::new(AtomicBool::new(false));

    // The first run: memory is away for the whole run, so the turn's records wait.
    let mut first = InAppAgent::with_kit(
        parts(&archive_model(), &sheet(), &clock()),
        InAppKit::default()
            .memory(memory!(service, up))
            .audit_to(AuditTo::Memory)
            .audit_file(AuditFile::open(&file).expect("no file yet")),
    )
    .expect("agent");
    first.ask("archive the digest").await.expect("turn");
    let waiting = first.audit();
    assert!(!waiting.is_empty(), "memory was away: the records wait");
    assert_eq!(first.take_audit_fault(), None);
    assert!(file.exists(), "and they are in the file");
    drop(first);

    // The second run reads them back and writes them when memory answers.
    let up = Arc::new(AtomicBool::new(true));
    let held_over = AuditFile::open(&file).expect("the file");
    assert_eq!(held_over.waiting(), waiting.as_slice());
    let mut second = InAppAgent::with_kit(
        parts(&ScriptedInfer::new(vec![]), &TestSheet::default(), &clock()),
        InAppKit::default()
            .memory(memory!(service, up))
            .audit_to(AuditTo::Memory)
            .audit_file(held_over),
    )
    .expect("agent");
    assert_eq!(second.audit(), waiting, "queued again at the start");
    let reader = memory!(service, up);
    assert_eq!(held(&reader, "docket.call").await, 0);

    let report = second.flush_audit().await;
    assert_eq!(report.queued, 0, "{report:?}");
    assert!(report.flushed.written >= waiting.len(), "{report:?}");
    assert!(held(&reader, "docket.call").await >= 1, "written to memory");
    assert!(second.audit().is_empty());
    assert!(!file.exists(), "nothing waits, so no file");
    assert_eq!(second.take_audit_fault(), None);
}

#[tokio::test]
async fn what_memory_still_cannot_take_stays_in_the_file_for_the_run_after() {
    let service = Arc::new(fake_service(ScriptedConsolidator::default()));
    let dir = tempfile::tempdir().expect("scratch");
    let file = dir.path().join("audit-queue.json");
    let down = Arc::new(AtomicBool::new(false));

    let open = |model: &ScriptedInfer, sheet: &TestSheet| {
        InAppAgent::with_kit(
            parts(model, sheet, &clock()),
            InAppKit::default()
                .memory(memory!(service, down))
                .audit_to(AuditTo::Memory)
                .audit_file(AuditFile::open(&file).expect("file")),
        )
        .expect("agent")
    };
    let mut first = open(&archive_model(), &sheet());
    first.ask("archive the digest").await.expect("turn");
    let waiting = first.audit().len();
    drop(first);

    let mut second = open(&ScriptedInfer::new(vec![]), &TestSheet::default());
    let report = second.flush_audit().await;
    assert_eq!(report.flushed.written, 0, "{report:?}");
    assert_eq!(report.queued, waiting, "{report:?}");
    drop(second);
    assert_eq!(
        AuditFile::open(&file).expect("file").waiting().len(),
        waiting,
        "still there for the run after"
    );
}

#[test]
fn the_file_keeps_only_the_newest_records_up_to_its_bound() {
    let dir = tempfile::tempdir().expect("scratch");
    let file = dir.path().join("queue.json");
    let records = demo_records(5);
    AuditFile::fresh(&file)
        .bounded(2)
        .save(&records)
        .expect("saved");
    assert_eq!(
        AuditFile::open(&file).expect("file").waiting(),
        &records[3..],
        "the newest two"
    );
}

#[test]
fn a_damaged_file_is_a_typed_error_and_fresh_starts_over_it() {
    let dir = tempfile::tempdir().expect("scratch");
    let file = dir.path().join("queue.json");
    std::fs::write(&file, "{ not a list").expect("write");
    assert!(matches!(
        AuditFile::open(&file),
        Err(AuditFileError::Corrupt { .. })
    ));
    let fresh = AuditFile::fresh(&file);
    assert!(fresh.waiting().is_empty());
    fresh.save(&demo_records(1)).expect("replaced");
    assert_eq!(AuditFile::open(&file).expect("file").waiting().len(), 1);
}

#[test]
fn a_write_that_cannot_happen_is_a_typed_error_and_leaves_no_half_file() {
    let dir = tempfile::tempdir().expect("scratch");
    let blocker = dir.path().join("blocker");
    std::fs::write(&blocker, "a file, not a directory").expect("write");
    let under_a_file = AuditFile::fresh(blocker.join("queue.json"));
    assert!(matches!(
        under_a_file.save(&demo_records(1)),
        Err(AuditFileError::Write { .. })
    ));

    let file = dir.path().join("queue.json");
    AuditFile::fresh(&file)
        .save(&demo_records(3))
        .expect("saved");
    let leftovers: Vec<_> = std::fs::read_dir(dir.path())
        .expect("dir")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        !leftovers.iter().any(|n| n.ends_with(".tmp")),
        "the temporary file was renamed away: {leftovers:?}"
    );
}

#[test]
fn an_empty_queue_removes_the_file_and_a_missing_file_is_nothing_waiting() {
    let dir = tempfile::tempdir().expect("scratch");
    let file = dir.path().join("queue.json");
    assert!(AuditFile::open(&file).expect("none").waiting().is_empty());
    let handle = AuditFile::fresh(&file);
    handle.save(&demo_records(2)).expect("saved");
    assert!(file.exists());
    handle.save(&[]).expect("emptied");
    assert!(!file.exists());
    handle.save(&[]).expect("nothing to remove is fine");
}

/// `n` audit records, each its own (a confirmation answered at second `i`).
fn demo_records(n: usize) -> Vec<docket_core::AuditRecord> {
    (0..n)
        .map(|i| docket_core::AuditRecord::Confirm {
            at: UnixSeconds(i64::try_from(i).unwrap_or(0)),
            id: docket_core::ConfirmId::parse("c-1").expect("id"),
            answer: docket_core::ConfirmAnswerKind::Ended(docket_core::ConfirmEnd::Dismissed),
            input: None,
        })
        .collect()
}
