//! The seam to memory and to the event log, as the router asks them: traits and plain values
//! only. Implementations live elsewhere (`docket-memory` over almanac, the fakes).

use almanac_core::{MemoryReply, MemoryRequest};
use prov::SpaceId;
use std::future::Future;

use crate::AuditRecord;

/// Why a call to an app or to memory got no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum LinkFault {
    /// The app is not running and could not be started.
    #[error("unavailable")]
    Unavailable,
    /// It did not answer in time.
    #[error("timed out")]
    Timeout,
    /// It answered something that is not the protocol.
    #[error("malformed answer")]
    Malformed,
}

/// The event log, as the router sees it.
pub trait EventSink: Send + Sync {
    /// Appends one record: never content.
    fn append(&self, record: AuditRecord);
}

/// Memory, reached as `Caller::Router` (the router acts for the companion's session, in the
/// Space of the invocation; the companion never calls memory itself).
pub trait MemoryLink: Send + Sync {
    /// One memory request, answered by memoryd.
    fn ask(
        &self,
        request: MemoryRequest,
    ) -> impl Future<Output = Result<MemoryReply, LinkFault>> + Send;

    /// Deletes what memory keeps for a Space that was removed, rather than moving it to any other
    /// Space. A link without that operation says [`SpaceMemories::Kept`], which leaves the
    /// records where they are, unreachable from a Space that is gone.
    fn erase_space(&self, _space: &SpaceId) -> impl Future<Output = SpaceMemories> + Send {
        async { SpaceMemories::Kept }
    }
}

/// What became of a removed Space's memories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceMemories {
    /// Memory was left as it was.
    Kept,
    /// Memory deleted them.
    Deleted,
}
