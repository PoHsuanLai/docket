//! The write-ahead record of a session: what the router has decided to remember, in order,
//! before it is durable, and the writer that makes it so.
//!
//! The router mutates its state under one synchronous lock, so it cannot await a log there.
//! Each mutation that belongs on the record is queued on the session (`Wal::note`) in the order
//! it happened; `Router::flush` (an async step outside the lock, see `durable.rs`) appends the
//! queue through one `Writer` per session, which owns the position, the durable taint and the
//! rule that an untrusted handle never reaches the log before a taint entry.

use docket_core::CallId;
use docket_session::{
    LogFault, Seq, SessionEntry, SessionLog, Taint as Written, TaintCause, TaintNote,
};
use prov::{Integrity, SessionId};
use std::sync::Arc;

/// Whether, and how far, a session is on the durable record.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Wal {
    /// Not recorded: an implicit session (an app, a terminal, an MCP client) or one restored
    /// for display only.
    #[default]
    Off,
    /// Recorded; these entries wait to be appended.
    On {
        /// In order, oldest first.
        pending: Vec<SessionEntry>,
        /// The taint already queued or appended: a `Taint` entry is due only above it.
        taint: Written,
    },
}

/// What is queued that must not be lost to a reply: a taint, or a handle of untrusted text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reveals {
    /// Nothing queued is a reveal.
    Nothing,
    /// A taint entry or an untrusted handle is waiting to be made durable.
    Untrusted,
}

impl Wal {
    /// A recorded session with nothing queued, at `taint`.
    pub fn on(taint: Written) -> Self {
        Wal::On {
            pending: Vec::new(),
            taint,
        }
    }

    /// Queues an entry (a session that is not recorded drops it).
    pub fn note(&mut self, entry: SessionEntry) {
        if let Wal::On { pending, taint } = self {
            if matches!(entry, SessionEntry::Taint(_)) {
                *taint = Written::Tainted;
            }
            pending.push(entry);
        }
    }

    /// Queues a taint entry when the session is not yet tainted on the record.
    pub fn note_taint(&mut self, cause: TaintCause, at_call: Option<CallId>) {
        if matches!(
            self,
            Wal::On {
                taint: Written::Clean,
                ..
            }
        ) {
            self.note(SessionEntry::Taint(TaintNote { cause, at_call }));
        }
    }

    /// Marks the record tainted without queueing (a writer already appended the entry).
    pub fn tainted(&mut self) {
        if let Wal::On { taint, .. } = self {
            *taint = Written::Tainted;
        }
    }

    /// Takes everything queued.
    pub fn take(&mut self) -> Vec<SessionEntry> {
        match self {
            Wal::On { pending, .. } => std::mem::take(pending),
            Wal::Off => Vec::new(),
        }
    }

    /// Puts entries back at the front of the queue (an append failed): order is kept.
    pub fn requeue(&mut self, mut entries: Vec<SessionEntry>) {
        if let Wal::On { pending, .. } = self {
            entries.append(pending);
            *pending = entries;
        }
    }

    /// Drops a queued `Call` entry that never ran (the call was refused before it began).
    pub fn retract_call(&mut self, call: CallId) {
        if let Wal::On { pending, .. } = self {
            pending.retain(|e| !matches!(e, SessionEntry::Call(open) if open.call == call));
        }
    }

    /// Whether anything is queued.
    pub fn is_queued(&self) -> bool {
        matches!(self, Wal::On { pending, .. } if !pending.is_empty())
    }

    /// Whether what is queued is a reveal.
    pub fn reveals(&self) -> Reveals {
        let untrusted = match self {
            Wal::On { pending, .. } => pending.iter().any(|e| match e {
                SessionEntry::Taint(_) => true,
                SessionEntry::Handle(h) => h.label.integrity == Integrity::Untrusted,
                _ => false,
            }),
            Wal::Off => false,
        };
        if untrusted {
            Reveals::Untrusted
        } else {
            Reveals::Nothing
        }
    }

    /// Whether the session is on the record.
    pub fn is_on(&self) -> bool {
        matches!(self, Wal::On { .. })
    }
}

/// Whether the writer may still append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Health {
    /// It may.
    Sound,
    /// The store holds another position than the writer's: another writer, or an append that
    /// landed unseen. Nothing more is written until the session is restored from the log.
    Diverged(Seq),
}

/// The one writer of a session's log: its next position, the taint on the record, its health.
#[derive(Debug)]
pub(crate) struct Writer {
    pub(crate) next: Seq,
    pub(crate) taint: Written,
    health: Health,
}

/// A writer shared by the requests of one session, taken in turn (never held with the router's
/// state lock).
pub(crate) type Lane = Arc<futures_util::lock::Mutex<Writer>>;

impl Writer {
    /// The writer of a session whose log is empty.
    pub(crate) fn fresh() -> Self {
        Self::at(Seq(0), Written::Clean)
    }

    /// The writer of a log that holds `next` entries and the taint `taint`.
    pub(crate) fn at(next: Seq, taint: Written) -> Self {
        Self {
            next,
            taint,
            health: Health::Sound,
        }
    }

    /// Appends `entry` after, if it needs one, the taint entry it must not precede.
    pub(crate) async fn put<L: SessionLog>(
        &mut self,
        log: &L,
        session: &SessionId,
        entry: SessionEntry,
    ) -> Result<(), LogFault> {
        let untrusted =
            matches!(&entry, SessionEntry::Handle(h) if h.label.integrity == Integrity::Untrusted);
        if untrusted && self.taint == Written::Clean {
            let note = TaintNote {
                cause: TaintCause::UntrustedReveal,
                at_call: None,
            };
            self.append(log, session, &SessionEntry::Taint(note))
                .await?;
        }
        self.append(log, session, &entry).await
    }

    /// Appends one entry at the writer's position; the position moves only on an ack.
    pub(crate) async fn append<L: SessionLog>(
        &mut self,
        log: &L,
        session: &SessionId,
        entry: &SessionEntry,
    ) -> Result<(), LogFault> {
        if let Health::Diverged(expected) = self.health {
            return Err(LogFault::OutOfOrder { expected });
        }
        match log.append(session, self.next, entry).await {
            Ok(_) => {
                self.next = self.next.next();
                if matches!(entry, SessionEntry::Taint(_)) {
                    self.taint = Written::Tainted;
                }
                Ok(())
            }
            Err(LogFault::OutOfOrder { expected }) => {
                self.health = Health::Diverged(expected);
                Err(LogFault::OutOfOrder { expected })
            }
            Err(other) => Err(other),
        }
    }
}
