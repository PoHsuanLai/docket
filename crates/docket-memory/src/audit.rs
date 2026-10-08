//! The audit log's way to memory: the queue the router fills (`QueuedSink`) is drained by the
//! host (a timer in intentd, the end of a turn in the in-app agent), each record becomes an
//! almanac `Record` (`record_of`) in the Space it happened in, and each Space's records go in one
//! batch as the router's own `Caller`, through any `MemoryLink`.
//!
//! When memory is away the records are not lost and not unbounded: what could not be written
//! goes back in the queue (`QueuedSink::restore`), which drops its oldest records past its limit
//! and counts them. `AuditState::flush` says what changed (`Report`) and prints nothing: the host
//! decides what to tell the person.

use crate::record::{record_of, space_named_by};
use crate::sink::QueuedSink;
use almanac_core::{MemoryReply, MemoryRequest, Refusal};
use docket_core::{AuditRecord, CallId};
use docket_router::{LinkFault, MemoryLink};
use prov::SpaceId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// How many calls the log remembers the Space of, so a review that is recorded after its call's
/// batch still lands beside it.
const CALLS_REMEMBERED: usize = 2048;

/// What one pass over the queue came to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flushed {
    /// Records memory took.
    pub written: usize,
    /// Records put back because memory was away, locked or busy.
    pub waiting: usize,
    /// Records memory refused for good, or that were dropped because the queue was full.
    pub lost: usize,
}

/// Whether memory answered the last time it was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
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

/// What one flush came to, for the host to tell the person (or not).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// The counts.
    pub flushed: Flushed,
    /// Whether memory answered before this flush and after it.
    pub link: (Link, Link),
    /// Records dropped for want of room since the last flush.
    pub overflow: usize,
    /// Records still queued after the flush.
    pub queued: usize,
    /// Spaces memory refused records for, the first time each is refused, with what it said.
    pub refused: Vec<(SpaceId, String)>,
}

/// The state of a log that writes into memory: the Space of each call it has seen, whether
/// memory answered last time, and which refused Spaces were already reported.
#[derive(Debug)]
pub struct AuditState {
    calls: Calls,
    link: Link,
    /// The Spaces memory refused records for, already reported.
    told: BTreeSet<SpaceId>,
    fresh: Vec<(SpaceId, String)>,
}

impl Default for AuditState {
    fn default() -> Self {
        Self {
            calls: Calls::default(),
            link: Link::Up,
            told: BTreeSet::new(),
            fresh: Vec::new(),
        }
    }
}

impl AuditState {
    /// The Spaces memory refused records for, each reported once.
    pub fn refused_spaces(&self) -> &BTreeSet<SpaceId> {
        &self.told
    }

    /// Takes what the queue holds and writes it into `memory`. `place` says where a record that
    /// names no Space of its own belongs (a breaker trip, from the router's session table); the
    /// Space of the call it reviews comes first, then `desktop`.
    pub async fn flush<L: MemoryLink>(
        &mut self,
        memory: &L,
        sink: &QueuedSink,
        place: impl Fn(&AuditRecord) -> Option<SpaceId>,
    ) -> Report {
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
        let mut flushed = self.write(memory, placed, sink).await;
        let overflow = usize::try_from(sink.dropped()).unwrap_or(usize::MAX);
        flushed.lost += overflow;
        Report {
            flushed,
            link: (before, self.link),
            overflow,
            queued: sink.len(),
            refused: std::mem::take(&mut self.fresh),
        }
    }

    /// Writes one Space's records at a time, each Space in the order its records were queued.
    /// The first Space that cannot be written and every Space after it go back in the queue.
    async fn write<L: MemoryLink>(
        &mut self,
        memory: &L,
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
            match self.write_space(memory, &space, mine).await {
                Done::Settled { written, lost } => {
                    report.written += written;
                    report.lost += lost;
                }
                Done::Waiting {
                    back,
                    written,
                    lost,
                } => {
                    report.written += written;
                    report.lost += lost;
                    waiting.extend(back);
                }
            }
        }
        waiting.sort_by_key(|(i, _)| *i);
        report.waiting = waiting.len();
        sink.restore(waiting.into_iter().map(|(_, r)| r).collect());
        report
    }

    async fn write_space<L: MemoryLink>(
        &mut self,
        memory: &L,
        space: &SpaceId,
        records: Vec<(usize, SpaceId, AuditRecord)>,
    ) -> Done {
        let events: Vec<_> = records
            .iter()
            .map(|(_, _, r)| record_of(r, space))
            .collect();
        let count = events.len();
        match memory.ask(MemoryRequest::RecordBatch(events)).await {
            Ok(MemoryReply::RecordedBatch(..) | MemoryReply::Ok | MemoryReply::Recorded(_)) => {
                self.link = Link::Up;
                Done::Settled {
                    written: count,
                    lost: 0,
                }
            }
            Ok(MemoryReply::Refused(Refusal::SpaceLocked | Refusal::Busy)) => {
                self.link = Link::Up;
                Done::keep(records)
            }
            // A batch memory refuses may hold one record it dislikes: try them one at a time so
            // the rest are kept.
            Ok(MemoryReply::Refused(_)) => self.write_each(memory, space, records).await,
            Ok(_) | Err(LinkFault::Malformed) => {
                self.link = Link::Up;
                self.tell_refused(space, "an unexpected reply");
                Done::Settled {
                    written: 0,
                    lost: count,
                }
            }
            Err(LinkFault::Unavailable | LinkFault::Timeout) => {
                self.link = Link::Down;
                Done::keep(records)
            }
        }
    }

    async fn write_each<L: MemoryLink>(
        &mut self,
        memory: &L,
        space: &SpaceId,
        records: Vec<(usize, SpaceId, AuditRecord)>,
    ) -> Done {
        let (mut written, mut lost) = (0, 0);
        let mut waiting = Vec::new();
        let mut records = records.into_iter();
        while let Some((i, _, record)) = records.next() {
            match memory
                .ask(MemoryRequest::Record(record_of(&record, space)))
                .await
            {
                // A lock is memory answering: the link stays up and the rest wait for the next flush.
                Ok(MemoryReply::Refused(Refusal::SpaceLocked | Refusal::Busy)) => {
                    self.link = Link::Up;
                    waiting.push((i, record));
                    waiting.extend(records.by_ref().map(|(i, _, r)| (i, r)));
                }
                Err(LinkFault::Unavailable | LinkFault::Timeout) => {
                    self.link = Link::Down;
                    waiting.push((i, record));
                    waiting.extend(records.by_ref().map(|(i, _, r)| (i, r)));
                }
                Ok(MemoryReply::Refused(why)) => {
                    self.tell_refused(space, &format!("{:?}", why));
                    lost += 1;
                }
                Err(why) => {
                    self.tell_refused(space, &format!("{why:?}"));
                    lost += 1;
                }
                Ok(_) => written += 1,
            }
        }
        match waiting.is_empty() {
            false => Done::Waiting {
                back: waiting,
                written,
                lost,
            },
            true => Done::Settled { written, lost },
        }
    }

    /// Notes, once per Space, that memory refused its records: they are counted as lost from
    /// then on, not reported again.
    fn tell_refused(&mut self, space: &SpaceId, why: &str) {
        if self.told.insert(space.clone()) {
            self.fresh.push((space.clone(), why.to_owned()));
        }
    }
}

enum Done {
    Settled {
        written: usize,
        lost: usize,
    },
    Waiting {
        back: Vec<(usize, AuditRecord)>,
        written: usize,
        lost: usize,
    },
}

impl Done {
    /// Every record goes back in the queue; none was written or lost.
    fn keep(records: Vec<(usize, SpaceId, AuditRecord)>) -> Self {
        Self::Waiting {
            back: records.into_iter().map(|(i, _, r)| (i, r)).collect(),
            written: 0,
            lost: 0,
        }
    }
}
