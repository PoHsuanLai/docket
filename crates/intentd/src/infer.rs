//! intentd's models, over inferd: the reviewer's stages and the policy writer ask through
//! porter-client sessions, and the quarantined reader is a client of `Reader1`. The model logic
//! is `docket-models`' (portable, over any transport); what is here is the bus constructor.

use docket_dbus::InferLink;
use docket_dbus::tap::Tap;
use docket_models::TransportModel;
use porter_client::Transport;
use porter_infer::{
    ChatReply, ChatRequest, ChatSink, EmbedReply, EmbedRequest, Model, ModelCard, ModelError,
};

pub use crate::reader_client::ReaderClient;
pub use crate::writer::InferdWriter;

/// inferd over the session bus: the one link every daemon of this repo makes, on `connection`
/// (see `docket_dbus::inferd_transport`).
pub fn inferd_transport(connection: &docket_dbus::BusConnection, tap: Tap) -> InferLink {
    docket_dbus::inferd_transport(connection, tap)
}

/// A chat model reached through an inferd session.
#[derive(Debug)]
pub struct InferdModel<T: Transport>(TransportModel<T>);

impl<T: Transport> InferdModel<T> {
    /// Asks through `transport`; `card` says who answers.
    pub fn new(transport: T, card: ModelCard) -> Self {
        Self(TransportModel::new(transport, card))
    }
}

impl InferdModel<InferLink> {
    /// Asks inferd over the session bus; `card` says who answers.
    pub fn on_bus(connection: &docket_dbus::BusConnection, card: ModelCard, tap: Tap) -> Self {
        Self::new(inferd_transport(connection, tap), card)
    }
}

impl<T: Transport> Model for InferdModel<T> {
    fn card(&self) -> &ModelCard {
        self.0.card()
    }

    async fn chat(
        &self,
        request: &ChatRequest,
        sink: &mut impl ChatSink,
    ) -> Result<ChatReply, ModelError> {
        self.0.chat(request, sink).await
    }

    async fn embed(&self, request: &EmbedRequest) -> Result<EmbedReply, ModelError> {
        self.0.embed(request).await
    }
}
