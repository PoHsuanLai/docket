//! The four seams an app may choose an answer for, and where the audit goes. The default kit is
//! the stubs (consent in memory, no memory, no policy writer, no reader, audit in the buffer), so
//! `InAppAgent::new` is what it was; each method swaps one for a real part:
//!
//! - grants: [`FileGrantStore`](crate::FileGrantStore) (consent survives a restart);
//! - memory: `docket_memory::AlmanacMemory` over an almanac-client transport;
//! - writer: `docket_models::TransportWriter` over a porter-client transport;
//! - reader: `docket_reader::TransportReader` over a porter-client transport.

use crate::seams::{NoMemory, NoReader, NoWriter, SessionGrants};

/// Where the audit trail of a turn goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AuditTo {
    /// Into the buffer the app drains (`InAppAgent::audit`, `AuditBuffer::drain`).
    #[default]
    Buffer,
    /// Into memory as almanac records at the end of each turn, the task's episode among them;
    /// what memory could not take stays queued for the next turn.
    Memory,
}

/// The seams an app chooses, and where the audit goes.
#[derive(Debug)]
pub struct InAppKit<G = SessionGrants, Y = NoMemory, W = NoWriter, D = NoReader> {
    /// The consent store.
    pub grants: G,
    /// Memory.
    pub memory: Y,
    /// The task-policy writer.
    pub writer: W,
    /// The quarantined reader.
    pub reader: D,
    /// Where the audit trail goes.
    pub audit: AuditTo,
}

impl Default for InAppKit {
    fn default() -> Self {
        Self {
            grants: SessionGrants::default(),
            memory: NoMemory,
            writer: NoWriter,
            reader: NoReader,
            audit: AuditTo::default(),
        }
    }
}

impl<G, Y, W, D> InAppKit<G, Y, W, D> {
    /// Keeps consent in `grants`.
    pub fn grants<G2>(self, grants: G2) -> InAppKit<G2, Y, W, D> {
        InAppKit {
            grants,
            memory: self.memory,
            writer: self.writer,
            reader: self.reader,
            audit: self.audit,
        }
    }

    /// Asks and remembers through `memory`.
    pub fn memory<Y2>(self, memory: Y2) -> InAppKit<G, Y2, W, D> {
        InAppKit {
            grants: self.grants,
            memory,
            writer: self.writer,
            reader: self.reader,
            audit: self.audit,
        }
    }

    /// Derives each task's policy with `writer`.
    pub fn writer<W2>(self, writer: W2) -> InAppKit<G, Y, W2, D> {
        InAppKit {
            grants: self.grants,
            memory: self.memory,
            writer,
            reader: self.reader,
            audit: self.audit,
        }
    }

    /// Reads untrusted text with `reader`.
    pub fn reader<D2>(self, reader: D2) -> InAppKit<G, Y, W, D2> {
        InAppKit {
            grants: self.grants,
            memory: self.memory,
            writer: self.writer,
            reader,
            audit: self.audit,
        }
    }

    /// Sends the audit trail to `audit`.
    pub fn audit_to(self, audit: AuditTo) -> Self {
        Self { audit, ..self }
    }
}
