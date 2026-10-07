//! The router's event sink. The router is synchronous about it: `append` queues, and a task of
//! the host (the daemon, or the in-app agent after a turn) drains the queue into memory, so a slow memoryd never stalls a call.
//!
//! The queue is bounded. While memoryd is away the daemon puts what it could not write back
//! (`restore`), and when the queue is full the oldest record is dropped and counted (`dropped`),
//! so a desktop that never has a memoryd cannot grow intentd without end.

use docket_core::AuditRecord;
use docket_router::EventSink;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// How many records wait for memoryd before the oldest are dropped.
pub const QUEUE_LIMIT: usize = 4096;

#[derive(Debug, Default)]
struct Queue {
    records: VecDeque<AuditRecord>,
    dropped: u64,
}

/// A sink that queues records for the daemon to write. Clones share one queue.
#[derive(Debug, Clone)]
pub struct QueuedSink {
    queue: Arc<Mutex<Queue>>,
    limit: usize,
}

impl Default for QueuedSink {
    fn default() -> Self {
        Self::bounded(QUEUE_LIMIT)
    }
}

impl QueuedSink {
    /// An empty queue of the usual size.
    pub fn new() -> Self {
        Self::default()
    }

    /// An empty queue that keeps at most `limit` records.
    pub fn bounded(limit: usize) -> Self {
        Self {
            queue: Arc::default(),
            limit,
        }
    }

    fn locked(&self) -> MutexGuard<'_, Queue> {
        self.queue.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Takes everything queued so far, oldest first.
    pub fn drain(&self) -> Vec<AuditRecord> {
        std::mem::take(&mut self.locked().records).into()
    }

    /// A copy of what is queued, oldest first, leaving it queued.
    pub fn snapshot(&self) -> Vec<AuditRecord> {
        self.locked().records.iter().cloned().collect()
    }

    /// Puts records that could not be written back in front of what was queued since, oldest
    /// first. What no longer fits is the oldest, and is counted.
    pub fn restore(&self, records: Vec<AuditRecord>) {
        let mut queue = self.locked();
        for record in records.into_iter().rev() {
            queue.records.push_front(record);
        }
        let over = queue.records.len().saturating_sub(self.limit);
        queue.records.drain(..over);
        queue.dropped += over as u64;
    }

    /// How many records wait.
    pub fn len(&self) -> usize {
        self.locked().records.len()
    }

    /// Whether nothing waits.
    pub fn is_empty(&self) -> bool {
        self.locked().records.is_empty()
    }

    /// How many records were dropped for want of room since the last call.
    pub fn dropped(&self) -> u64 {
        std::mem::take(&mut self.locked().dropped)
    }
}

impl EventSink for QueuedSink {
    fn append(&self, record: AuditRecord) {
        let mut queue = self.locked();
        queue.records.push_back(record);
        if queue.records.len() > self.limit {
            queue.records.pop_front();
            queue.dropped += 1;
        }
    }
}
