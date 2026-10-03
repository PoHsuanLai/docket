//! intentd's models, over inferd: the reviewer's stages and the policy writer ask through
//! porter-client sessions, and the quarantined reader is a client of `Reader1`. Every request
//! says its data class and `Usage::Interactive`; a refusal from inferd is an error that asks
//! the person, never an allow.

use porter_client::{AnyTransport, Transport};
use porter_core::capability::{LlmFeature, Modality};
use porter_core::need::{EmbedNeed, LlmNeed};
use porter_core::{Need, Tokens};
use porter_infer::{
    ChatReply, ChatRequest, ChatSink, ClientFrame, EmbedReply, EmbedRequest, Flow, InferEvent,
    InferReply, InferRequest, InferSession, Knob, MessagePart, Model, ModelCard, ModelError,
    ReplyShape,
};
use std::collections::BTreeSet;

pub use crate::reader_client::ReaderClient;
pub use crate::writer::InferdWriter;

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

/// Drops every event.
pub(crate) struct Discard;

impl ChatSink for Discard {
    fn event(&mut self, _event: InferEvent) -> Flow {
        Flow::Continue
    }
}

/// What a chat needs of the model that answers: chat, structured output when the reply has a
/// shape, tools when it has any, and room for the prompt and the reply.
pub(crate) fn chat_need(request: &ChatRequest) -> Need {
    let mut features = BTreeSet::from([LlmFeature::Chat]);
    if request.shape != ReplyShape::Text {
        features.insert(LlmFeature::StructuredOutput);
    }
    if !request.tools.is_empty() {
        features.insert(LlmFeature::Tools);
    }
    Need::Llm(LlmNeed {
        features,
        context: context_of(request),
    })
}

/// A rough size of the prompt and the reply it asks for, in tokens: three bytes a token for the
/// text, plus the reply's own limit (or a page when the model is left to its default).
fn context_of(request: &ChatRequest) -> Tokens {
    let bytes: usize = request
        .messages
        .iter()
        .flat_map(|m| &m.parts)
        .map(|part| match part {
            MessagePart::Text(text) => text.len(),
            MessagePart::Image(_)
            | MessagePart::ToolCall(_)
            | MessagePart::ToolResult(_)
            | MessagePart::Thought(_) => 0,
        })
        .sum();
    let reply = match request.control.max_output {
        Knob::Set(Tokens(n)) => n,
        Knob::Off => 1024,
    };
    Tokens(
        u32::try_from(bytes / 3)
            .unwrap_or(u32::MAX)
            .saturating_add(reply),
    )
}

/// Runs one request to its end on `session`: events go to `sink`, the last one is the reply.
///
/// A daemon that refuses a session writes the refusal and hangs up, so the write can fail while
/// the answer is already waiting: a failed write is ignored and the answer is read first; only
/// a session with nothing to read is unreachable.
pub(crate) async fn turn<S: InferSession>(
    session: &mut S,
    request: InferRequest,
    sink: &mut impl ChatSink,
) -> Result<InferReply, ModelError> {
    let _ = session.send(ClientFrame::Request(request)).await;
    let mut stopped = false;
    loop {
        match session.next().await {
            Ok(InferEvent::Finished(reply)) => return Ok(reply),
            Ok(event) => {
                if sink.event(event) == Flow::Stop && !stopped {
                    stopped = true;
                    let _ = session.send(ClientFrame::Cancel).await;
                }
            }
            Err(_) => return Err(ModelError::Unreachable),
        }
    }
}

/// What a reply that is not the one asked for is as a `Model` error: a refusal of inferd is the
/// model refusing (the reviewer's cascade turns every one into asking the person), a failed call
/// is its error, and an answer of another kind is unreadable.
pub(crate) fn unwanted(reply: InferReply) -> ModelError {
    match reply {
        InferReply::Refused(_) => ModelError::Refused,
        InferReply::Failed(why) => why,
        InferReply::Cancelled => ModelError::Unreachable,
        _ => ModelError::Unparseable,
    }
}

/// The chat reply of a turn's last event.
pub(crate) fn chat_of(reply: InferReply) -> Result<ChatReply, ModelError> {
    match reply {
        InferReply::Chat(chat) => Ok(chat),
        other => Err(unwanted(other)),
    }
}

/// The embeddings of a turn's last event.
pub(crate) fn embed_of(reply: InferReply) -> Result<EmbedReply, ModelError> {
    match reply {
        InferReply::Embed(embedded) => Ok(embedded),
        other => Err(unwanted(other)),
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
        let mut session = self
            .transport
            .open(&chat_need(request), request.class, request.tier)
            .await
            .map_err(|_| ModelError::Unreachable)?;
        chat_of(turn(&mut session, InferRequest::Chat(request.clone()), sink).await?)
    }

    async fn embed(&self, request: &EmbedRequest) -> Result<EmbedReply, ModelError> {
        let need = Need::Embeddings(EmbedNeed {
            dims: request.dims,
            modalities: BTreeSet::from([Modality::Text]),
        });
        let mut session = self
            .transport
            .open(&need, request.class, porter_core::Tier::Fast)
            .await
            .map_err(|_| ModelError::Unreachable)?;
        let reply = turn(
            &mut session,
            InferRequest::Embed(request.clone()),
            &mut Discard,
        )
        .await?;
        embed_of(reply)
    }
}
