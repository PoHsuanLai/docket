//! The router is told when the app's turn is over, whichever way it ends. A turn that is
//! answered, fails or is cancelled ends through [`TurnWatch::end`]. A turn whose future is
//! dropped ends through `Drop`, which polls the send once: the router is in this process, so the
//! call finishes at once. If it does not, the router ends the turn by time
//! (`agent.checkpoints.turn_max_s`).

use docket_client::{Intents, Transport};
use docket_core::{TurnEnd, TurnId};
use futures_util::FutureExt;
use prov::SessionId;

/// A recorded turn that has not ended.
pub(crate) struct TurnWatch<'a, T: Transport> {
    intents: &'a Intents<T>,
    held: Option<(SessionId, TurnId)>,
}

impl<'a, T: Transport> TurnWatch<'a, T> {
    /// `turn` of `session` has been recorded.
    pub(crate) fn begin(intents: &'a Intents<T>, session: SessionId, turn: TurnId) -> Self {
        Self {
            intents,
            held: Some((session, turn)),
        }
    }

    /// Says the turn ended as `how`.
    pub(crate) async fn end(mut self, how: TurnEnd) {
        if let Some((session, turn)) = self.held.take() {
            let _ = self.intents.session_turn_ended(session, turn, how).await;
        }
    }
}

impl<T: Transport> Drop for TurnWatch<'_, T> {
    fn drop(&mut self) {
        if let Some((session, turn)) = self.held.take() {
            let send = self
                .intents
                .session_turn_ended(session, turn, TurnEnd::Cancelled);
            let _ = send.now_or_never();
        }
    }
}
