//! The person's side of `org.quire.Companion1`, as a seam: open a conversation, ask, follow the
//! answer object until it settles, close. [`CompanionTransport`] is what `quire-do ask` is
//! written against; [`DbusCompanion`] is the session bus.
//! Recording the person's turn is not here: that is `Intents1.Session.Turn`, which the caller
//! makes through [`docket_client::Intents`] before it asks.

use companion_wire::{AnswerWire, AskWire};
use docket_client::TransportError;
use docket_core::{SessionOpen, SessionOpened};
use prov::SessionId;
use std::future::Future;

mod bus;

pub use bus::{BusAnswer, DbusCompanion};

/// One answer object, followed: the view as it stands, then each change.
pub trait Follow: Send {
    /// The next view: the first call answers the current one, later calls wait for a change.
    /// `None` when the answer object is gone.
    fn next(&mut self) -> impl Future<Output = Result<Option<AnswerWire>, TransportError>> + Send;
}

/// One carrier of the companion's conversation calls.
pub trait CompanionTransport: Send + Sync {
    /// What follows an answer.
    type Answer: Follow;

    /// `Open`: a conversation.
    fn open(
        &self,
        open: &SessionOpen,
    ) -> impl Future<Output = Result<SessionOpened, TransportError>> + Send;

    /// `Ask`: the companion takes the turn the router recorded; the answer is followed from here,
    /// subscribed before the loop can say anything.
    fn ask(
        &self,
        ask: &AskWire,
    ) -> impl Future<Output = Result<Self::Answer, TransportError>> + Send;

    /// `Close`: the conversation ends.
    fn close(&self, session: &SessionId)
    -> impl Future<Output = Result<(), TransportError>> + Send;
}
