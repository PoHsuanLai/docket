//! Making the session record durable, outside the router's lock.
//!
//! The router decides under one synchronous lock and queues entries on the session (`Wal`);
//! these steps await the log with no lock held. Three moves:
//!
//! - `flush`: append what is queued, in order, through the session's writer. A failure leaves
//!   the entries queued and the position where it was; the next flush tries again.
//! - `ahead_of_reveal`: the write-ahead rule. Before untrusted text is shown to a model or held
//!   as a handle, the taint entry is appended and acked; if it cannot be, the caller refuses the
//!   reveal and the session stays as it was.
//! - `settle`: the backstop at the end of a request. Everything queued is flushed; a reply that
//!   carries a reveal whose entry could not be made durable is withheld.

use crate::router::Router;
use crate::seams::Seams;
use crate::wal::{Lane, Reveals, Writer};
use docket_core::{CallId, CallRefusal, IntentsReply, WireRefusal};
use docket_session::{LogFault, SessionEntry, Taint as Written, TaintCause, TaintNote};
use prov::SessionId;
use std::sync::Arc;

/// How a flush ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Flush {
    /// Everything queued is on the record.
    Durable,
    /// Some entries could not be appended and wait; `Reveals` says whether one is a reveal.
    Kept(Reveals),
}

impl<S: Seams> Router<S> {
    /// The writer of a recorded session, made when first needed; `None` for a session that is
    /// not recorded.
    fn lane(&self, id: &SessionId) -> Option<Lane> {
        let mut st = self.locked();
        if !st.sessions.get(id)?.wal.is_on() {
            return None;
        }
        Some(Arc::clone(st.lanes.entry(id.clone()).or_insert_with(
            || Arc::new(futures_util::lock::Mutex::new(Writer::fresh())),
        )))
    }

    /// Appends everything queued for `id` and everything minted since, through `writer`, which
    /// the caller holds. On a failure the failed entry and those after it go back to the front.
    async fn drain(&self, id: &SessionId, writer: &mut Writer) -> Result<(), LogFault> {
        loop {
            let batch = {
                let mut st = self.locked();
                let Some(record) = st.sessions.get_mut(id) else {
                    return Ok(());
                };
                record.gather();
                record.wal.take()
            };
            if batch.is_empty() {
                return Ok(());
            }
            for (done, entry) in batch.iter().enumerate() {
                if let Err(fault) = writer.put(self.seams.log(), id, entry.clone()).await {
                    if let Some(record) = self.locked().sessions.get_mut(id) {
                        record.wal.requeue(batch[done..].to_vec());
                    }
                    return Err(fault);
                }
            }
        }
    }

    /// Appends what is queued for `id`.
    pub(crate) async fn flush(&self, id: &SessionId) -> Flush {
        let Some(lane) = self.lane(id) else {
            return Flush::Durable;
        };
        let mut writer = lane.lock().await;
        match self.drain(id, &mut writer).await {
            Ok(()) => Flush::Durable,
            Err(_) => Flush::Kept(
                self.locked()
                    .sessions
                    .get(id)
                    .map_or(Reveals::Nothing, |r| r.wal.reveals()),
            ),
        }
    }

    /// The write-ahead rule: the session's taint is on the record, acked, before untrusted text
    /// reaches a model or is held as a handle. `Err` means it could not be, and the caller must
    /// not reveal.
    pub(crate) async fn ahead_of_reveal(
        &self,
        id: &SessionId,
        at_call: Option<CallId>,
    ) -> Result<(), CallRefusal> {
        let Some(lane) = self.lane(id) else {
            return Ok(());
        };
        let mut writer = lane.lock().await;
        if writer.taint == Written::Tainted {
            return Ok(());
        }
        let note = SessionEntry::Taint(TaintNote {
            cause: TaintCause::UntrustedReveal,
            at_call,
        });
        writer
            .append(self.seams.log(), id, &note)
            .await
            .map_err(|_| CallRefusal::NotRecorded)?;
        if let Some(record) = self.locked().sessions.get_mut(id) {
            record.wal.tainted();
        }
        Ok(())
    }

    /// Ends a request: flushes every session with something queued. A reply is withheld when
    /// a reveal in it is not on the record.
    pub(crate) async fn settle(&self, reply: IntentsReply) -> IntentsReply {
        let due: Vec<SessionId> = {
            let mut st = self.locked();
            st.sessions
                .iter_mut()
                .filter_map(|(id, record)| {
                    if !record.wal.is_on() {
                        // Not recorded: nothing will ask for the labels.
                        record.handles.take_fresh();
                        return None;
                    }
                    (record.wal.is_queued() || record.handles.has_fresh()).then(|| id.clone())
                })
                .collect()
        };
        let mut kept = Reveals::Nothing;
        for id in due {
            if self.flush(&id).await == Flush::Kept(Reveals::Untrusted) {
                kept = Reveals::Untrusted;
            }
        }
        match (kept, carries_text(&reply)) {
            (Reveals::Untrusted, true) => {
                IntentsReply::Refused(WireRefusal::Call(CallRefusal::NotRecorded))
            }
            _ => reply,
        }
    }
}

/// Whether a reply can carry text of a session's own to its caller: only these are withheld. A
/// halt, a state read or a turn's receipt is not held back by a log that is down.
fn carries_text(reply: &IntentsReply) -> bool {
    match reply {
        IntentsReply::Performed(result) => result.is_ok(),
        IntentsReply::Resolved(_)
        | IntentsReply::Read(_)
        | IntentsReply::Context(_)
        | IntentsReply::Recalled(_)
        | IntentsReply::Inbox(_)
        | IntentsReply::Text(_)
        | IntentsReply::Handles(_) => true,
        _ => false,
    }
}
