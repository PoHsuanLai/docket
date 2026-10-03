//! intentd's models, over inferd: the reviewer's stages and the policy writer ask through
//! porter-client sessions, and the quarantined reader is a client of `Reader1`. Every request
//! says its data class and `Usage::Interactive`; a refusal from inferd is an error that asks
//! the person, never an allow.

use docket_core::{
    ActionCard, PolicyWriter, Reader, ReaderAsk, ReaderError, ReviewError, TaskPolicy, UserTurn,
    Value,
};
use porter_client::{AnyTransport, Transport};
use porter_infer::{
    ChatReply, ChatRequest, ChatSink, EmbedReply, EmbedRequest, Model, ModelCard, ModelError,
};
use prov::{Quarantined, SpaceId, TaskId};

/// inferd over the session bus: the one link every daemon of this repo makes, on `connection`
/// (see `docket_dbus::inferd_transport`).
pub fn inferd_transport(connection: &docket_dbus::BusConnection) -> AnyTransport {
    docket_dbus::inferd_transport(connection)
}

/// A chat model reached through an inferd session.
#[derive(Debug)]
pub struct InferdModel<T: Transport> {
    transport: T,
    card: ModelCard,
}

impl<T: Transport> InferdModel<T> {
    /// Asks through `transport`; `card` says who answers.
    pub fn new(transport: T, card: ModelCard) -> Self {
        Self { transport, card }
    }
}

impl InferdModel<AnyTransport> {
    /// Asks inferd over the session bus; `card` says who answers.
    pub fn on_bus(connection: &docket_dbus::BusConnection, card: ModelCard) -> Self {
        Self::new(inferd_transport(connection), card)
    }
}

impl<T: Transport> Model for InferdModel<T> {
    fn card(&self) -> &ModelCard {
        &self.card
    }

    async fn chat(
        &self,
        request: &ChatRequest,
        sink: &mut impl ChatSink,
    ) -> Result<ChatReply, ModelError> {
        let _ = (&self.transport, request, sink);
        todo!(
            "InferdModel::chat: Transport::open(Need::Llm, request.class, request.tier), send ClientFrame::Request(Chat), forward events to the sink until Finished"
        )
    }

    async fn embed(&self, request: &EmbedRequest) -> Result<EmbedReply, ModelError> {
        let _ = (&self.transport, request);
        todo!("InferdModel::embed: Transport::open(Need::Embeddings, ..)")
    }
}

/// The policy writer: reads the person's turns and the action catalogue, never content.
#[derive(Debug)]
pub struct InferdWriter<T: Transport> {
    transport: T,
}

impl<T: Transport> InferdWriter<T> {
    /// Asks through `transport`.
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl InferdWriter<AnyTransport> {
    /// Asks inferd over the session bus.
    pub fn on_bus(connection: &docket_dbus::BusConnection) -> Self {
        Self::new(inferd_transport(connection))
    }
}

impl<T: Transport> PolicyWriter for InferdWriter<T> {
    async fn derive(
        &self,
        task: &TaskId,
        turns: &[UserTurn],
        catalogue: &[ActionCard],
        space: &SpaceId,
    ) -> Result<TaskPolicy, ReviewError> {
        let _ = (&self.transport, task, turns, catalogue, space);
        todo!(
            "InferdWriter::derive: ReplyShape::Json of the policy record from the turns and the catalogue alone; on failure there is no policy and every non-read call is outside"
        )
    }
}

/// The quarantined reader, over `org.quire.Reader1`.
#[derive(Debug, Clone)]
pub struct ReaderClient {
    connection: docket_dbus::BusConnection,
}

impl ReaderClient {
    /// Calls readerd over `connection`.
    pub fn new(connection: docket_dbus::BusConnection) -> Self {
        Self { connection }
    }
}

impl Reader for ReaderClient {
    async fn extract(
        &self,
        ask: ReaderAsk,
        inputs: Vec<Quarantined<String>>,
    ) -> Result<Value, ReaderError> {
        let _ = (&self.connection, ask, inputs);
        todo!(
            "ReaderClient::extract: Reader1.Extract(session, ask); the inputs stay in intentd's handle table and readerd resolves them through Session.Resolve"
        )
    }
}
