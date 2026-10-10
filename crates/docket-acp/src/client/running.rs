//! The turn the router recorded for this host and the agent has not finished yet. Whichever way
//! the host leaves the turn (the agent answered, failed or was cancelled, the session closed, the
//! host was dropped), the router is told with `Session.TurnEnded`, so a restore of the agent's
//! files is refused for exactly as long as the agent is at work.
//!
//! A guard dropped without `end` (the host's task was dropped, or the host itself) sends the word
//! from a task of its own, as `Cancelled`. If there is no runtime to spawn on, the router ends
//! the turn by time (`agent.checkpoints.turn_max_s`).

use super::court::Court;
use docket_core::{TurnEnd, TurnId};
use prov::SessionId;

/// What the guard sends, and to whom.
struct Held<C: Court> {
    court: C,
    session: SessionId,
    turn: TurnId,
}

/// One running turn; sends `Session.TurnEnded` when it ends or is dropped.
pub(crate) struct RunningTurn<C: Court> {
    held: Option<Held<C>>,
}

impl<C: Court> RunningTurn<C> {
    /// `turn` of `session` began at the router.
    pub(crate) fn begin(court: C, session: SessionId, turn: TurnId) -> Self {
        Self {
            held: Some(Held {
                court,
                session,
                turn,
            }),
        }
    }

    /// Says the turn ended as `how`, and waits until the router has heard it.
    pub(crate) async fn end(mut self, how: TurnEnd) {
        if let Some(mut held) = self.held.take() {
            held.court.turn_ended(&held.session, held.turn, how).await;
        }
    }
}

impl<C: Court> Drop for RunningTurn<C> {
    fn drop(&mut self) {
        let Some(mut held) = self.held.take() else {
            return;
        };
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                held.court
                    .turn_ended(&held.session, held.turn, TurnEnd::Cancelled)
                    .await;
            });
        }
    }
}
