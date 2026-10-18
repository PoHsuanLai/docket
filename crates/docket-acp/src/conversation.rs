//! The turn in a helper's conversation (design note `swarm.md`, section 7.2). An editor may speak
//! in the one conversation each helper has, but the turn is not run here: the helper's planner,
//! model, pool and reach live with the daemon that keeps the helper, and only it may name the
//! helper as the one asking. This seam carries the person's words there and brings the helper's
//! words back.
//!
//! The answer is the reply of the call, not a stream. The daemon runs the turn under the helper's
//! own gate, sheets included (a sheet goes to the person on the desktop, naming the helper; the
//! editor answers none), and replies with the words when the turn ends. Following the session's
//! events would hand the editor what the helper read; the hub's change feed carries no words. One
//! reply carries only what the helper chose to say. While it is awaited the connection reads no
//! further lines, so a `session/cancel` during such a turn is heard after it.

use crate::helpers::HelperRefusal;
use docket_core::UserTurn;
use prov::SessionId;
use std::future::Future;

/// Where the person's words in a helper's conversation are taken to be answered.
pub trait ConversationTurns: Send {
    /// The person's `turn` in the helper's conversation `session`: the helper's words when its
    /// turn ends, or the plain sentence of why it could not answer.
    fn say(
        &mut self,
        session: &SessionId,
        turn: UserTurn,
    ) -> impl Future<Output = Result<String, HelperRefusal>> + Send;
}
