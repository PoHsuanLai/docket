//! The router's event sink. The router is synchronous about it: `append` queues, and a task of
//! the daemon drains the queue into memoryd, so a slow memoryd never stalls a call.

use docket_core::AuditRecord;
use docket_router::EventSink;
use std::sync::Mutex;

/// A sink that queues records for the daemon to write.
#[derive(Debug, Default)]
pub struct QueuedSink {
    queue: Mutex<Vec<AuditRecord>>,
}

impl QueuedSink {
    /// An empty queue.
    pub fn new() -> Self {
        Self::default()
    }

    /// Takes everything queued so far, oldest first.
    pub fn drain(&self) -> Vec<AuditRecord> {
        match self.queue.lock() {
            Ok(mut queue) => std::mem::take(&mut *queue),
            Err(poisoned) => std::mem::take(&mut *poisoned.into_inner()),
        }
    }
}

impl EventSink for QueuedSink {
    fn append(&self, record: AuditRecord) {
        match self.queue.lock() {
            Ok(mut queue) => queue.push(record),
            Err(poisoned) => poisoned.into_inner().push(record),
        }
    }
}
