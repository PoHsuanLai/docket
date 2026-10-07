//! The router's seams for one app: the app's provider, its sheet, its reviewer and clock, an
//! in-memory consent store and audit buffer, and the three seams that have no portable answer
//! yet (memory, the quarantined reader, the task-policy writer): each says "unavailable", which
//! the router already handles as "work with less".

use crate::link::ProviderLink;
use crate::sheet::{ConfirmSheet, SheetConfirmer};
use action_review::Reviewer;
use almanac_core::{MemoryReply, MemoryRequest};
use docket_client::{ContextSource, IntentProvider};
use docket_core::{
    ActionCard, ActionGrant, AuditRecord, PolicyWriter, Reader, ReaderAsk, ReaderError,
    ReviewError, TaskPolicy, UserTurn, Value,
};
use docket_router::{Clock, EventSink, GrantStore, LinkFault, MemoryLink, Seams};
use prov::{Quarantined, SessionId, SpaceId, TaskId};
use std::sync::Mutex;

fn held<T, R>(m: &Mutex<T>, f: impl FnOnce(&mut T) -> R) -> R {
    match m.lock() {
        Ok(mut g) => f(&mut g),
        Err(p) => f(&mut p.into_inner()),
    }
}

/// The consent the person gave in this process: gone when the process ends. An app that wants
/// "always" to last stores `grants()` itself and replays them with [`SessionGrants::with`].
#[derive(Debug, Default)]
pub struct SessionGrants(Mutex<Vec<ActionGrant>>);

impl SessionGrants {
    /// A store that starts with these grants.
    pub fn with(grants: Vec<ActionGrant>) -> Self {
        Self(Mutex::new(grants))
    }
}

impl GrantStore for SessionGrants {
    fn grants(&self) -> Vec<ActionGrant> {
        held(&self.0, |g| g.clone())
    }
    fn record(&self, grant: ActionGrant) {
        held(&self.0, |g| g.push(grant));
    }
}

/// The audit records of this process, in order; the app drains them into its own log.
#[derive(Debug, Default)]
pub struct AuditBuffer(Mutex<Vec<AuditRecord>>);

impl AuditBuffer {
    /// Takes every record appended so far.
    pub fn drain(&self) -> Vec<AuditRecord> {
        held(&self.0, std::mem::take)
    }

    /// A copy of every record appended so far.
    pub fn records(&self) -> Vec<AuditRecord> {
        held(&self.0, |r| r.clone())
    }
}

impl EventSink for AuditBuffer {
    fn append(&self, record: AuditRecord) {
        held(&self.0, |r| r.push(record));
    }
}

/// No memory: every request is unavailable, so the working set has no recall, primer or
/// episodes and nothing is remembered.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoMemory;

impl MemoryLink for NoMemory {
    async fn ask(&self, _request: MemoryRequest) -> Result<MemoryReply, LinkFault> {
        Err(LinkFault::Unavailable)
    }
}

/// No quarantined reader: a `quire_read` step fails, and the loop ends the turn as failed.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoReader;

impl Reader for NoReader {
    async fn extract(
        &self,
        _session: &SessionId,
        _ask: ReaderAsk,
        _inputs: Vec<Quarantined<String>>,
    ) -> Result<Value, ReaderError> {
        Err(ReaderError::ModelUnavailable)
    }
}

/// No task-policy writer: a task has no policy derived from the person's words, and the default
/// policies and the reviewer decide alone.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoWriter;

impl PolicyWriter for NoWriter {
    async fn derive(
        &self,
        _task: &TaskId,
        _turns: &[UserTurn],
        _catalogue: &[ActionCard],
        _space: &SpaceId,
    ) -> Result<TaskPolicy, ReviewError> {
        Err(ReviewError::Unavailable)
    }
}

/// Every seam of an in-app router. Public so an app reads what its sheet, store and buffer hold.
#[derive(Debug)]
pub struct InAppSeams<P, C, T, R, K> {
    /// The app's own provider.
    pub link: ProviderLink<P, C>,
    /// The app's sheet, with the clock that stamps its receipts.
    pub confirmer: SheetConfirmer<T, K>,
    /// The reviewer cascade.
    pub reviewer: R,
    /// The consent store.
    pub grants: SessionGrants,
    /// The audit buffer.
    pub sink: AuditBuffer,
    /// The clock, as the router races its deadlines.
    pub clock: K,
    /// No memory.
    pub memory: NoMemory,
    /// No policy writer.
    pub writer: NoWriter,
    /// No reader.
    pub reader: NoReader,
}

impl<P, C, T, R, K> Seams for InAppSeams<P, C, T, R, K>
where
    P: IntentProvider,
    C: ContextSource,
    T: ConfirmSheet,
    R: Reviewer,
    K: Clock + Clone,
{
    type Link = ProviderLink<P, C>;
    type Confirm = SheetConfirmer<T, K>;
    type Review = R;
    type Grants = SessionGrants;
    type Sink = AuditBuffer;
    type Time = K;
    type Memory = NoMemory;
    type Writer = NoWriter;
    type Reading = NoReader;

    fn link(&self) -> &Self::Link {
        &self.link
    }
    fn confirmer(&self) -> &Self::Confirm {
        &self.confirmer
    }
    fn reviewer(&self) -> &R {
        &self.reviewer
    }
    fn grants(&self) -> &SessionGrants {
        &self.grants
    }
    fn sink(&self) -> &AuditBuffer {
        &self.sink
    }
    fn clock(&self) -> &K {
        &self.clock
    }
    fn memory(&self) -> &NoMemory {
        &self.memory
    }
    fn writer(&self) -> &NoWriter {
        &self.writer
    }
    fn reader(&self) -> &NoReader {
        &self.reader
    }
}
