//! The small fakes: a fixed clock, a recording event sink and an in-memory consent store.

use docket_core::{ActionGrant, AuditRecord};
use docket_router::{Clock, EventSink, GrantStore};
use prov::UnixSeconds;
use std::sync::Mutex;

/// A clock that always says the same instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixedClock(pub UnixSeconds);

impl Clock for FixedClock {
    fn now(&self) -> UnixSeconds {
        self.0
    }
}

/// An event sink that keeps every record, in order.
#[derive(Debug, Default)]
pub struct RecordingSink {
    records: Mutex<Vec<AuditRecord>>,
}

impl RecordingSink {
    /// An empty sink.
    pub fn new() -> Self {
        Self::default()
    }

    /// Everything appended so far.
    pub fn records(&self) -> Vec<AuditRecord> {
        self.records.lock().map(|r| r.clone()).unwrap_or_default()
    }

    /// Forgets everything appended.
    pub fn clear(&self) {
        if let Ok(mut records) = self.records.lock() {
            records.clear();
        }
    }
}

impl EventSink for RecordingSink {
    fn append(&self, record: AuditRecord) {
        if let Ok(mut records) = self.records.lock() {
            records.push(record);
        }
    }
}

/// A consent store in memory.
#[derive(Debug, Default)]
pub struct MemoryGrants {
    grants: Mutex<Vec<ActionGrant>>,
}

impl MemoryGrants {
    /// A store with no grants.
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets every grant.
    pub fn clear(&self) {
        if let Ok(mut grants) = self.grants.lock() {
            grants.clear();
        }
    }

    /// A store that starts with these grants.
    pub fn with(grants: Vec<ActionGrant>) -> Self {
        Self {
            grants: Mutex::new(grants),
        }
    }
}

impl GrantStore for MemoryGrants {
    fn grants(&self) -> Vec<ActionGrant> {
        self.grants.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn record(&self, grant: ActionGrant) {
        if let Ok(mut grants) = self.grants.lock() {
            grants.push(grant);
        }
    }
}
