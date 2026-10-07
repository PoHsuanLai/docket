//! `SessionLog`: the seam to the durable store. docket-memory implements it over almanac in S1;
//! until almanac's session-log asks land (FINDINGS: kind-prefix `Recent` with a cursor, a durable
//! append ack, not-for-recall) this crate asks nothing of it but two calls.
//!
//! The writer owns the per-session position (`Seq`, from 0, carried in the stored body): an
//! append names the position it writes, and a store that holds a different next position
//! refuses with `OutOfOrder`, so two writers cannot interleave and a gap is never silent.

use crate::codec::Logged;
use crate::entry::{Seq, SessionEntry};
use porter_core::Count;
use prov::SessionId;
use std::future::Future;

/// Why an append or a page failed. A taint entry that is refused for any of these refuses the
/// reveal it guards (fail closed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LogFault {
    /// The store holds a different next position.
    #[error("the log's next position is {expected:?}")]
    OutOfOrder {
        /// What it wanted.
        expected: Seq,
    },
    /// The store will not remember (full, or the Space is set to forget).
    #[error("the log refused the entry")]
    Refused,
    /// The store did not answer.
    #[error("the log is unavailable")]
    Unavailable,
    /// The entry could not be written as JSON.
    #[error("the entry could not be encoded")]
    Encode,
}

/// The acknowledgement of a durable append: the entry is stored, in order, and will be read back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Appended {
    /// The position written.
    pub seq: Seq,
}

/// How many rows a page holds at most.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PageSize(pub Count);

/// A page of one session's rows, oldest first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogPage {
    /// The rows.
    pub rows: Vec<Logged>,
    /// Where to ask for the next page; `None` at the end.
    pub next: Option<Seq>,
}

/// A log shared by several holders (a router and the one that replaces it after a restart) is a
/// log.
impl<L: SessionLog> SessionLog for std::sync::Arc<L> {
    fn append(
        &self,
        session: &SessionId,
        seq: Seq,
        entry: &SessionEntry,
    ) -> impl Future<Output = Result<Appended, LogFault>> + Send {
        L::append(self, session, seq, entry)
    }

    fn page(
        &self,
        session: &SessionId,
        from: Option<Seq>,
        size: PageSize,
    ) -> impl Future<Output = Result<LogPage, LogFault>> + Send {
        L::page(self, session, from, size)
    }

    fn sessions(&self) -> impl Future<Output = Result<Vec<SessionId>, LogFault>> + Send {
        L::sessions(self)
    }
}

/// The durable log of sessions.
pub trait SessionLog: Send + Sync {
    /// Appends `entry` as position `seq` of `session`, and answers only when it is durable.
    fn append(
        &self,
        session: &SessionId,
        seq: Seq,
        entry: &SessionEntry,
    ) -> impl Future<Output = Result<Appended, LogFault>> + Send;

    /// The rows of `session` from position `from` (the start when `None`), oldest first, at most
    /// `size`. Bodies come back as `decode` reads them: what cannot be read is a row, not a fault.
    fn page(
        &self,
        session: &SessionId,
        from: Option<Seq>,
        size: PageSize,
    ) -> impl Future<Output = Result<LogPage, LogFault>> + Send;

    /// Every session that has an opening in the log, oldest first: what a daemon reads at start
    /// so that a session it opens next never takes the name of one the log already holds.
    fn sessions(&self) -> impl Future<Output = Result<Vec<SessionId>, LogFault>> + Send;
}
