//! What a session's state changes put on its record: the one place a transition and its entry
//! meet, so a change that belongs on the record cannot be made without queueing it.

use crate::session::{SessionEffect, SessionEvent, SessionState, session_step};
use crate::state::SessionRecord;
use docket_session::{SessionEntry, TaintCause};

impl SessionRecord {
    /// Applies a session event and queues what it did to the record: a close, a taint.
    pub(crate) fn apply(&mut self, event: SessionEvent) -> Vec<SessionEffect> {
        let was_closed = matches!(self.state, SessionState::Closed(_));
        let (next, effects) = session_step(self.state, event);
        self.state = next;
        match (was_closed, next, event) {
            (false, SessionState::Closed(cause), _) => {
                self.wal.note(SessionEntry::Closed(cause.into()));
            }
            (false, _, SessionEvent::UntrustedReveal) => {
                self.wal.note_taint(TaintCause::UntrustedReveal, None);
            }
            _ => {}
        }
        effects
    }

    /// Queues the handles minted since the last gathering: label, shape and source.
    pub(crate) fn gather(&mut self) {
        for label in self.handles.take_fresh() {
            self.wal.note(SessionEntry::Handle(label));
        }
    }
}
