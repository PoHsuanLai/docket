//! The memory seam over almanac-client transports that are not the bus: the service hosted in
//! this process (almanac-fake's backend) and no service at all. The D-Bus path is covered by
//! intentd's `audit.rs` through its re-exports.

use almanac_client::{Absent, InProcess};
use almanac_core::{BodyMode, Caller, MemoryReply, MemoryRequest, RecentQuery, TrustFilter};
use almanac_fake::{ScriptedConsolidator, fake_service};
use docket_core::{AuditRecord, HaltCause};
use docket_memory::{AlmanacMemory, AuditState, Link, QueuedSink};
use docket_router::{EventSink, LinkFault, MemoryLink};
use porter_core::Count;
use prov::{SpaceId, SpaceScope, UnixSeconds};
use std::sync::Arc;

fn work() -> SpaceId {
    SpaceId::parse("work").expect("space")
}

fn halt(at: i64) -> AuditRecord {
    AuditRecord::Halt {
        at: UnixSeconds(at),
        scope: SpaceScope::Any,
        cause: HaltCause::KillChord,
    }
}

fn queued(n: i64) -> QueuedSink {
    let sink = QueuedSink::new();
    (1..=n).for_each(|at| sink.append(halt(at)));
    sink
}

async fn stored(memory: &impl MemoryLink) -> usize {
    let recent = RecentQuery {
        since: UnixSeconds(0),
        kinds: Vec::new(),
        trust: TrustFilter::Any,
        limit: Count(50),
        bodies: BodyMode::Json,
    };
    match memory.ask(MemoryRequest::Recent(work(), recent)).await {
        Ok(MemoryReply::Recent(entries)) => entries.len(),
        other => panic!("recent: {other:?}"),
    }
}

#[tokio::test]
async fn records_reach_in_process_memory_in_the_space_the_host_names() {
    let service = Arc::new(fake_service(ScriptedConsolidator::default()));
    let memory = AlmanacMemory::over(InProcess::new(service, Caller::Router));
    let sink = queued(3);
    let mut state = AuditState::default();
    let report = state.flush(&memory, &sink, |_| Some(work())).await;
    assert_eq!(
        (
            report.flushed.written,
            report.flushed.waiting,
            report.flushed.lost
        ),
        (3, 0, 0)
    );
    assert_eq!(report.link, (Link::Up, Link::Up));
    assert!(sink.is_empty());
    assert_eq!(stored(&memory).await, 3);
}

#[tokio::test]
async fn no_memory_keeps_the_records_queued_and_the_next_flush_writes_them() {
    let sink = queued(2);
    let mut state = AuditState::default();
    let away = AlmanacMemory::over(Absent);
    assert_eq!(
        away.ask(MemoryRequest::Spaces).await.err(),
        Some(LinkFault::Unavailable)
    );
    let report = state.flush(&away, &sink, |_| Some(work())).await;
    assert_eq!(report.flushed.waiting, 2);
    assert_eq!(report.link, (Link::Up, Link::Down));
    assert_eq!(report.queued, 2, "nothing was lost");
    assert_eq!(
        sink.snapshot().len(),
        2,
        "and a snapshot leaves them queued"
    );

    let service = Arc::new(fake_service(ScriptedConsolidator::default()));
    let back = AlmanacMemory::over(InProcess::new(service, Caller::Router));
    let report = state.flush(&back, &sink, |_| Some(work())).await;
    assert_eq!(report.flushed.written, 2);
    assert_eq!(report.link, (Link::Down, Link::Up));
    assert_eq!(stored(&back).await, 2);
}

#[tokio::test]
async fn a_full_queue_drops_the_oldest_and_the_report_counts_them() {
    let sink = QueuedSink::bounded(2);
    (1..=4).for_each(|at| sink.append(halt(at)));
    let mut state = AuditState::default();
    let away = AlmanacMemory::over(Absent);
    let report = state.flush(&away, &sink, |_| Some(work())).await;
    assert_eq!(report.overflow, 2);
    assert_eq!(report.flushed.lost, 2);
    assert_eq!(report.queued, 2);
}
