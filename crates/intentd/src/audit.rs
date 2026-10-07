//! The audit log's way to memoryd. The mapping, the queue and the retry accounting are
//! `docket-memory`'s (portable, over any memory link); this is the daemon's log: it owns the
//! memoryd link and says on standard error when the link goes down and comes back, how many
//! records it had to drop, and which Space memoryd refused.

use almanac_client::Transport;
use docket_core::AuditRecord;
use docket_memory::{AlmanacMemory, AuditState, Link};
use prov::SpaceId;
use std::collections::BTreeSet;

pub use docket_memory::Flushed;

use docket_memory::QueuedSink;

/// Writes the router's records into memoryd.
#[derive(Debug)]
pub struct AuditLog<T: Transport> {
    memory: AlmanacMemory<T>,
    state: AuditState,
}

impl<T: Transport> AuditLog<T> {
    /// A log that writes through `transport`.
    pub fn over(transport: T) -> Self {
        Self {
            memory: AlmanacMemory::over(transport),
            state: AuditState::default(),
        }
    }

    /// The Spaces memoryd refused records for, each already said once on standard error.
    pub fn refused_spaces(&self) -> &BTreeSet<SpaceId> {
        self.state.refused_spaces()
    }

    /// Takes what the queue holds and writes it. `place` says where a record that names no
    /// Space of its own belongs (a breaker trip, from the router's session table); the Space of
    /// the call it reviews comes first, then `desktop`.
    pub async fn flush(
        &mut self,
        sink: &QueuedSink,
        place: impl Fn(&AuditRecord) -> Option<SpaceId>,
    ) -> Flushed {
        let report = self.state.flush(&self.memory, sink, place).await;
        match report.link {
            (Link::Up, Link::Down) => eprintln!(
                "intentd: memoryd is not answering: {} audit records wait for it",
                report.queued
            ),
            (Link::Down, Link::Up) => eprintln!(
                "intentd: memoryd answers again: {} audit records written",
                report.flushed.written
            ),
            (Link::Up, Link::Up) | (Link::Down, Link::Down) => {}
        }
        if report.overflow > 0 {
            eprintln!(
                "intentd: {} audit records were dropped (the queue was full)",
                report.overflow
            );
        }
        for (space, why) in &report.refused {
            eprintln!(
                "intentd: memoryd refused the audit records of Space {space} ({why}); they are dropped and counted in Control.State"
            );
        }
        report.flushed
    }
}
