//! The service behind `org.quire.Reader1`.

use crate::answer::answer_of;
use crate::host::ReaderHost;
use crate::request::reader_request;
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::{Reader, ReaderAsk, ReaderError, Value};
use porter_client::{AnyTransport, Transport as InferTransport};
use porter_core::capability::LlmFeature;
use porter_core::need::LlmNeed;
use porter_core::{Need, Tokens};
use porter_infer::{
    ClientFrame, InferEvent, InferReply, InferRequest, InferSession, ModelError, StopReason,
};
use prov::{Labelled, Quarantined, SessionId};
use std::collections::BTreeSet;

/// Reads for intentd: resolves handles through `Intents1.Session.Resolve` (the reader role),
/// asks the reader model through inferd (`Need::Llm` with structured output, no tools), and
/// answers a value that fits the schema.
#[derive(Debug)]
pub struct ReaderService<P: InferTransport, I: IntentsTransport> {
    host: ReaderHost,
    infer: P,
    intents: Intents<I>,
}

impl<I: IntentsTransport> ReaderService<AnyTransport, I> {
    /// A service over inferd on the session bus (`AnyTransport::Dbus`) and the router. Nothing is
    /// called here: inferd is found, and started by activation, at the first session.
    pub fn on_bus(
        host: ReaderHost,
        connection: &docket_dbus::BusConnection,
        intents: Intents<I>,
    ) -> Self {
        Self::new(host, docket_dbus::inferd_transport(connection), intents)
    }
}

impl<P: InferTransport, I: IntentsTransport> ReaderService<P, I> {
    /// A service over inferd and the router.
    pub fn new(host: ReaderHost, infer: P, intents: Intents<I>) -> Self {
        Self {
            host,
            infer,
            intents,
        }
    }

    /// One `Reader1.Extract(session, ask)`: resolves each handle of the ask in the session,
    /// then reads. A handle the router will not resolve (not this session's, or the caller is
    /// not the reader) fails the whole read: nothing is read from a part of the ask.
    pub async fn extract_in(
        &self,
        session: &SessionId,
        ask: ReaderAsk,
    ) -> Result<Value, ReaderError> {
        let mut inputs = Vec::with_capacity(ask.inputs.len());
        for handle in &ask.inputs {
            let text = self
                .intents
                .session_resolve(session.clone(), *handle)
                .await
                .map_err(|_| ReaderError::ModelUnavailable)?;
            inputs.push(Quarantined::new(Labelled {
                value: text,
                label: resolved_label(),
            }));
        }
        self.extract(ask, inputs).await
    }

    async fn read(
        &self,
        ask: &ReaderAsk,
        inputs: &[Labelled<String>],
    ) -> Result<Value, ReaderError> {
        // A schema with no shape in the structured-output vocabulary is refused before any
        // model is asked.
        ask.want.shape().map_err(ReaderError::OutOfSchema)?;
        let request = reader_request(ask, inputs);
        let need = Need::Llm(LlmNeed {
            features: BTreeSet::from([LlmFeature::Chat, LlmFeature::StructuredOutput]),
            context: Tokens(
                u32::try_from(inputs.iter().map(|i| i.value.len()).sum::<usize>() / 3)
                    .unwrap_or(u32::MAX)
                    .saturating_add(1024),
            ),
        });
        let mut session = self
            .infer
            .open(&need, request.class, request.tier)
            .await
            .map_err(|_| ReaderError::ModelUnavailable)?;
        // A daemon that refuses a session hangs up after writing the refusal: read first.
        let _ = session
            .send(ClientFrame::Request(InferRequest::Chat(request.clone())))
            .await;
        let reply = loop {
            match session.next().await {
                Ok(InferEvent::Finished(reply)) => break reply,
                Ok(_) => {}
                Err(_) => return Err(ReaderError::ModelUnavailable),
            }
        };
        match reply {
            InferReply::Chat(chat) => match chat.stop {
                StopReason::EndTurn | StopReason::StopSequence => answer_of(&chat.text, &ask.want),
                StopReason::MaxTokens | StopReason::ContentFilter | StopReason::ToolUse => {
                    Err(ReaderError::Unparseable)
                }
            },
            InferReply::Failed(ModelError::Refused) => Err(ReaderError::Refused),
            InferReply::Failed(ModelError::Unparseable | ModelError::Unreadable) => {
                Err(ReaderError::Unparseable)
            }
            InferReply::Failed(_)
            | InferReply::Refused(_)
            | InferReply::Cancelled
            | InferReply::Embed(_)
            | InferReply::CuaStep(_)
            | InferReply::Transcribed(_)
            | InferReply::Spoke(_) => Err(ReaderError::ModelUnavailable),
        }
    }
}

/// The label of text the router resolved for the reader. The router keeps each handle's own
/// label in its table and `Session.Resolve` answers the text alone, so the label here classes
/// nothing, and a request over unclassed text is sent as the person's own words: its floor is
/// this computer (see `class_of`).
fn resolved_label() -> prov::Label {
    prov::Label {
        integrity: prov::Integrity::Untrusted,
        confidentiality: prov::Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::new(),
    }
}

impl<P: InferTransport, I: IntentsTransport> Reader for ReaderService<P, I> {
    async fn extract(
        &self,
        ask: ReaderAsk,
        inputs: Vec<Quarantined<String>>,
    ) -> Result<Value, ReaderError> {
        let opened: Vec<Labelled<String>> = inputs.into_iter().map(|q| self.host.open(q)).collect();
        self.read(&ask, &opened).await
    }
}
