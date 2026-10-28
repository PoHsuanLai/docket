//! The mode of a conversation: Auto or Manual, set by the person and read afresh each time a call
//! is prepared, so a switch takes effect from the next step. A session that works for a
//! conversation (a worker, a helper's job, a screen run) has no mode of its own to follow: it
//! takes its parent's, through the chain.

use crate::router::Router;
use crate::seams::Seams;
use docket_context::state::RouterState;
use docket_permissions_core::{Mode, ModeRows};
use docket_wire::{IntentsReply, SessionModeAsk, WireRefusal};
use prov::SessionId;

impl<S: Seams> Router<S> {
    /// The mode a new conversation starts in: the person's setting, held at Manual by a floor
    /// that asks for more asking.
    pub(crate) fn start_mode(&self) -> Mode {
        self.floor.clamp_mode(self.agent_config().start_mode)
    }

    /// The mode a call of `session` is ruled under, and what that mode does without asking: the
    /// person's rows for the mode, with the rows the administrator's floor locks set to "asks".
    /// A call of no session, or of one the router does not hold, is ruled as a new conversation
    /// would be.
    pub(crate) fn mode_in_force(
        &self,
        st: &RouterState,
        session: Option<&SessionId>,
    ) -> (Mode, ModeRows) {
        let mode = self.floor.clamp_mode(
            session
                .and_then(|id| st.mode_of(id))
                .unwrap_or_else(|| self.agent_config().start_mode),
        );
        let rows = self.floor.clamp_rows(self.agent_config().rows.of(mode));
        (mode, rows)
    }

    /// `.Session.SetMode`: switches a conversation between Auto and Manual from the next step.
    /// Who may ask is the role table's (the shell and the control centre); a call already
    /// waiting on a sheet keeps its sheet.
    pub(crate) fn session_mode(&self, ask: SessionModeAsk) -> IntentsReply {
        if self.floor.holds_manual() {
            return IntentsReply::Refused(WireRefusal::Locked);
        }
        match self.locked().sessions.get_mut(&ask.session) {
            Some(record) => {
                record.mode = ask.mode;
                IntentsReply::Done
            }
            None => IntentsReply::Refused(WireRefusal::NoSuchSession),
        }
    }
}
