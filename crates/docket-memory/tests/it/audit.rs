//! The audit flush when memory refuses a whole batch and the one-by-one retry then meets a lock
//! or a missing service: what was written and lost before the stop still counts, and a lock is
//! memory answering while a missing service is the link down. Memory is a scripted fake.

use almanac_core::{MemoryReply, MemoryRequest, Refusal};
use docket_core::{AuditRecord, HaltCause};
use docket_memory::{AuditState, Link, QueuedSink};
use docket_router::{EventSink, LinkFault, MemoryLink};
use prov::{SpaceId, SpaceScope, UnixSeconds};
use std::collections::VecDeque;
use std::sync::Mutex;

/// Answers each request with the next scripted reply, whatever the request.
struct Script(Mutex<VecDeque<Result<MemoryReply, LinkFault>>>);

impl Script {
    fn of(replies: impl IntoIterator<Item = Result<MemoryReply, LinkFault>>) -> Self {
        Self(Mutex::new(replies.into_iter().collect()))
    }
}

impl MemoryLink for Script {
    async fn ask(&self, _request: MemoryRequest) -> Result<MemoryReply, LinkFault> {
        self.0
            .lock()
            .expect("script")
            .pop_front()
            .unwrap_or(Err(LinkFault::Unavailable))
    }
}

fn halt(at: i64) -> AuditRecord {
    AuditRecord::Halt {
        at: UnixSeconds(at),
        scope: SpaceScope::Any,
        cause: HaltCause::KillChord,
    }
}

fn three() -> QueuedSink {
    let sink = QueuedSink::new();
    (1..=3).for_each(|at| sink.append(halt(at)));
    sink
}

fn invalid() -> Result<MemoryReply, LinkFault> {
    Ok(MemoryReply::Refused(Refusal::Invalid("no".into())))
}

async fn flush_after_batch_refusal(
    then: [Result<MemoryReply, LinkFault>; 3],
) -> (docket_memory::Report, QueuedSink) {
    let memory = Script::of([invalid()].into_iter().chain(then));
    let sink = three();
    let mut state = AuditState::default();
    let report = state
        .flush(&memory, &sink, |_| Some(SpaceId::desktop()))
        .await;
    (report, sink)
}

#[tokio::test]
async fn a_lock_after_a_written_and_a_lost_record_keeps_both_counts_and_the_link_up() {
    let (report, sink) =
        flush_after_batch_refusal([Ok(MemoryReply::Ok), invalid(), Ok(refused_busy())]).await;
    let f = report.flushed;
    assert_eq!((f.written, f.lost, f.waiting), (1, 1, 1));
    assert_eq!(report.link.1, Link::Up);
    assert_eq!(sink.len(), 1);
}

#[tokio::test]
async fn a_missing_service_after_a_written_and_a_lost_record_keeps_both_counts_and_the_link_down() {
    let (report, sink) =
        flush_after_batch_refusal([Ok(MemoryReply::Ok), invalid(), Err(LinkFault::Unavailable)])
            .await;
    let f = report.flushed;
    assert_eq!((f.written, f.lost, f.waiting), (1, 1, 1));
    assert_eq!(report.link.1, Link::Down);
    assert_eq!(sink.len(), 1);
}

fn refused_busy() -> MemoryReply {
    MemoryReply::Refused(Refusal::Busy)
}
