//! The router hears that a native turn is over on every way out of its flight. A flight that
//! runs to its end says so itself (`disarm` follows the send). A flight dropped before that (the
//! host was dropped, or the session was left) is cut off, and this guard's `Drop` sends
//! `Session.TurnEnded` as `Cancelled` from the drop itself: the send is polled once, with no
//! runtime. That finishes at once when the router is in this process (docket-inapp), and is
//! best effort over a bus. If it does not finish, or the companion is busy with another turn
//! and cannot be locked, the router ends the turn by `Session.Close` or by time
//! (`agent.checkpoints.turn_max_s`).
//!
//! The guard is made before the flight locks the companion, so on a drop the lock is let go
//! first (locals drop in reverse) and the guard can take it to reach the router link.

use crate::backend::Core;
use docket_client::Transport as IntentsTransport;
use docket_core::{TurnEnd, TurnId};
use futures_util::FutureExt;
use porter_client::Transport as InferTransport;
use prov::SessionId;

/// A turn of `session` that has begun at the router and not ended.
pub(crate) struct Ending<P: InferTransport, I: IntentsTransport, K, S> {
    core: Core<P, I, K, S>,
    session: SessionId,
    turn: Option<TurnId>,
}

impl<P: InferTransport, I: IntentsTransport, K, S> Ending<P, I, K, S> {
    /// A guard for a turn of `session` that has not begun yet.
    pub(crate) fn new(core: Core<P, I, K, S>, session: SessionId) -> Self {
        Self {
            core,
            session,
            turn: None,
        }
    }

    /// `turn` began at the router: from now on a drop ends it.
    pub(crate) fn arm(&mut self, turn: TurnId) {
        self.turn = Some(turn);
    }

    /// The router has been told the turn is over.
    pub(crate) fn disarm(&mut self) {
        self.turn = None;
    }
}

impl<P: InferTransport, I: IntentsTransport, K, S> Drop for Ending<P, I, K, S> {
    fn drop(&mut self) {
        let Some(turn) = self.turn.take() else {
            return;
        };
        if let Some(companion) = self.core.try_lock() {
            let send = companion.intents.session_turn_ended(
                self.session.clone(),
                turn,
                TurnEnd::Cancelled,
            );
            let _ = send.now_or_never();
        }
    }
}
