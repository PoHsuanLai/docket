//! The audit log's way to memoryd: the queue the router fills (`QueuedSink`) is drained on a
//! timer, each record becomes an almanac `Record` (`record_of`) in the Space it happened in, and
//! each Space's records go in one batch as the router's own `Caller`.
//!
//! When memoryd is away the records are not lost and not unbounded: what could not be written
//! goes back in the queue (`QueuedSink::restore`), which drops its oldest records past its limit
//! and counts them. Intentd says so once when the link goes down and once when it comes back,
//! and says how many records it had to drop.

use crate::record::{record_of, space_named_by};
use crate::sink::QueuedSink;
use almanac_client::{ClientError, Memory, Transport};
use almanac_core::{MemoryReply, MemoryRequest, Refusal};
use docket_core::{AuditRecord, CallId};
use prov::SpaceId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// How many calls the log remembers the Space of, so a review that is recorded after its call's
/// batch still lands beside it.
const CALLS_REMEMBERED: usize = 2048;

/// What one pass over the queue came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flushed {
    /// Records memoryd took.
    pub written: usize,
    /// Records put back because memoryd was away, locked or busy.
    pub waiting: usize,
    /// Records memoryd refused for good, or that were dropped because the queue was full.
    pub lost: usize,
}

/// Whether memoryd answered the last time it was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Link {
    Up,
    Down,
}

/// Where the records of a call went: remembered for the records that name the call and no Space.
#[derive(Debug, Default)]
struct Calls {
    spaces: BTreeMap<CallId, SpaceId>,
    order: VecDeque<CallId>,
}

impl Calls {
    fn learn(&mut self, record: &AuditRecord) {
        if let AuditRecord::Call { call, space, .. } = record
            && self.spaces.insert(*call, space.clone()).is_none()
        {
            self.order.push_back(*call);
            while self.order.len() > CALLS_REMEMBERED {
                if let Some(old) = self.order.pop_front() {
                    self.spaces.remove(&old);
                }
            }
        }
    }

    fn of(&self, record: &AuditRecord) -> Option<&SpaceId> {
        match record {
            AuditRecord::Review { call, .. }
            | AuditRecord::Classified { call, .. }
            | AuditRecord::Delegation { call, .. } => self.spaces.get(call),
            _ => None,
        }
    }
}

/// Writes the router's records into memoryd.
#[derive(Debug)]
pub struct AuditLog<T: Transport> {
    memory: Memory<T>,
    calls: Calls,
    link: Link,
    /// The Spaces memoryd refused records for, already said on standard error.
    told: BTreeSet<SpaceId>,
}

impl<T: Transport> AuditLog<T> {
    /// A log that writes through `transport`.
    pub fn over(transport: T) -> Self {
        Self {
            memory: Memory::over(transport),
            calls: Calls::default(),
            link: Link::Up,
            told: BTreeSet::new(),
        }
    }

    /// The Spaces memoryd refused records for, each already said once on standard error.
    pub fn refused_spaces(&self) -> &BTreeSet<SpaceId> {
        &self.told
    }

    /// Takes what the queue holds and writes it. `place` says where a record that names no
    /// Space of its own belongs (a breaker trip, from the router's session table); the Space of
    /// the call it reviews comes first, then `desktop`.
    pub async fn flush(
        &mut self,
        sink: &QueuedSink,
        place: impl Fn(&AuditRecord) -> Option<SpaceId>,
    ) -> Flushed {
        let before = self.link;
        let batch = sink.drain();
        batch.iter().for_each(|r| self.calls.learn(r));
        let placed: Vec<(usize, SpaceId, AuditRecord)> = batch
            .into_iter()
            .enumerate()
            .map(|(i, r)| {
                let space = space_named_by(&r)
                    .cloned()
                    .or_else(|| self.calls.of(&r).cloned())
                    .or_else(|| place(&r))
                    .unwrap_or_else(SpaceId::desktop);
                (i, space, r)
            })
            .collect();
        let mut report = self.write(placed, sink).await;
        let overflow = usize::try_from(sink.dropped()).unwrap_or(usize::MAX);
        report.lost += overflow;
        self.say(before, &report, overflow, sink);
        report
    }

    /// Writes one Space's records at a time, each Space in the order its records were queued.
    /// The first Space that cannot be written and every Space after it go back in the queue.
    async fn write(
        &mut self,
        placed: Vec<(usize, SpaceId, AuditRecord)>,
        sink: &QueuedSink,
    ) -> Flushed {
        let mut report = Flushed::default();
        let mut spaces: Vec<SpaceId> = Vec::new();
        for (_, space, _) in &placed {
            if !spaces.contains(space) {
                spaces.push(space.clone());
            }
        }
        let mut waiting: Vec<(usize, AuditRecord)> = Vec::new();
        let mut rest = placed;
        for space in spaces {
            let (mine, others): (Vec<_>, Vec<_>) =
                rest.into_iter().partition(|(_, s, _)| *s == space);
            rest = others;
            if !waiting.is_empty() && self.link == Link::Down {
                waiting.extend(mine.into_iter().map(|(i, _, r)| (i, r)));
                continue;
            }
            match self.write_space(&space, mine).await {
                Done::Settled { written, lost } => {
                    report.written += written;
                    report.lost += lost;
                }
                Done::Waiting(back) => waiting.extend(back),
            }
        }
        waiting.sort_by_key(|(i, _)| *i);
        report.waiting = waiting.len();
        sink.restore(waiting.into_iter().map(|(_, r)| r).collect());
        report
    }

    async fn write_space(
        &mut self,
        space: &SpaceId,
        records: Vec<(usize, SpaceId, AuditRecord)>,
    ) -> Done {
        let events: Vec<_> = records
            .iter()
            .map(|(_, _, r)| record_of(r, space))
            .collect();
        let count = events.len();
        match self.memory.ask(MemoryRequest::RecordBatch(events)).await {
            Ok(MemoryReply::RecordedBatch(..) | MemoryReply::Ok | MemoryReply::Recorded(_)) => {
                self.link = Link::Up;
                Done::Settled {
                    written: count,
                    lost: 0,
                }
            }
            Ok(_) | Err(ClientError::Unexpected) => {
                self.link = Link::Up;
                self.tell_refused(space, "an unexpected reply");
                Done::Settled {
                    written: 0,
                    lost: count,
                }
            }
            Err(ClientError::Transport(_)) => {
                self.link = Link::Down;
                Done::Waiting(records.into_iter().map(|(i, _, r)| (i, r)).collect())
            }
            Err(ClientError::Refused(Refusal::SpaceLocked | Refusal::Busy)) => {
                self.link = Link::Up;
                Done::Waiting(records.into_iter().map(|(i, _, r)| (i, r)).collect())
            }
            // A batch memoryd refuses may hold one record it dislikes: try them one at a time so
            // the rest are kept.
            Err(ClientError::Refused(_)) => self.write_each(space, records).await,
        }
    }

    async fn write_each(
        &mut self,
        space: &SpaceId,
        records: Vec<(usize, SpaceId, AuditRecord)>,
    ) -> Done {
        let (mut written, mut lost) = (0, 0);
        let mut waiting = Vec::new();
        let mut records = records.into_iter();
        while let Some((i, _, record)) = records.next() {
            match self
                .memory
                .ask(MemoryRequest::Record(record_of(&record, space)))
                .await
            {
                Ok(_) => written += 1,
                Err(
                    ClientError::Transport(_)
                    | ClientError::Refused(Refusal::SpaceLocked | Refusal::Busy),
                ) => {
                    self.link = Link::Down;
                    waiting.push((i, record));
                    waiting.extend(records.by_ref().map(|(i, _, r)| (i, r)));
                }
                Err(why) => {
                    self.tell_refused(space, &format!("{why:?}"));
                    lost += 1;
                }
            }
        }
        match waiting.is_empty() {
            false => Done::Waiting(waiting),
            true => Done::Settled { written, lost },
        }
    }

    /// Says what changed: the link going down, coming back, and records lost.
    fn say(&self, before: Link, report: &Flushed, overflow: usize, sink: &QueuedSink) {
        match (before, self.link) {
            (Link::Up, Link::Down) => eprintln!(
                "intentd: memoryd is not answering: {} audit records wait for it",
                sink.len()
            ),
            (Link::Down, Link::Up) => eprintln!(
                "intentd: memoryd answers again: {} audit records written",
                report.written
            ),
            (Link::Up, Link::Up) | (Link::Down, Link::Down) => {}
        }
        if overflow > 0 {
            eprintln!("intentd: {overflow} audit records were dropped (the queue was full)");
        }
    }

    /// Says once per Space that memoryd refused its records: they are counted in
    /// `Control.State` (`audit_lost`) from then on, not repeated here.
    fn tell_refused(&mut self, space: &SpaceId, why: &str) {
        if self.told.insert(space.clone()) {
            eprintln!(
                "intentd: memoryd refused the audit records of Space {space} ({why}); they are dropped and counted in Control.State"
            );
        }
    }
}

enum Done {
    Settled { written: usize, lost: usize },
    Waiting(Vec<(usize, AuditRecord)>),
}
