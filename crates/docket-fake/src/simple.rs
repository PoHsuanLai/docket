//! The small fakes: a recording event sink and an in-memory consent store.

use docket_core::{
    ActionGrant, AuditRecord, Revocation, StandingGrant, StandingGrantId, held_with, held_without,
};
use docket_router::{EventSink, GrantStore};
use std::sync::Mutex;

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
    standing: Mutex<Vec<StandingGrant>>,
}

impl MemoryGrants {
    /// A store with no grants.
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets every grant, the standing ones too.
    pub fn clear(&self) {
        if let Ok(mut grants) = self.grants.lock() {
            grants.clear();
        }
        if let Ok(mut standing) = self.standing.lock() {
            standing.clear();
        }
    }

    /// A store that starts with these grants.
    pub fn with(grants: Vec<ActionGrant>) -> Self {
        Self {
            grants: Mutex::new(grants),
            standing: Mutex::default(),
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

    fn standing(&self) -> Vec<StandingGrant> {
        self.standing.lock().map(|g| g.clone()).unwrap_or_default()
    }

    fn add_standing(&self, grant: StandingGrant) {
        if let Ok(mut held) = self.standing.lock() {
            *held = held_with(std::mem::take(&mut held), grant);
        }
    }

    fn revoke_standing(&self, id: &StandingGrantId) -> Revocation {
        match self.standing.lock() {
            Ok(mut held) => {
                let (rest, done) = held_without(std::mem::take(&mut held), id);
                *held = rest;
                done
            }
            Err(_) => Revocation::NotHeld,
        }
    }
}
