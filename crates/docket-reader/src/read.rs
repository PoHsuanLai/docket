//! One read: the ask and the opened inputs go to the reader model through a porter-client
//! session (structured output, no tools), and the reply comes back as a value that fits the
//! ask's schema or as a typed error. `TransportReader` is the in-process `Reader`: it opens the
//! quarantined text itself, so an app that hosts the agent needs no second process.

use crate::answer::answer_of;
use crate::request::reader_request;
use docket_core::{Reader, ReaderAsk, ReaderError, Value};
use docket_models::{Discard, turn};
use porter_client::Transport;
use porter_core::capability::LlmFeature;
use porter_core::need::LlmNeed;
use porter_core::{Need, Tokens};
use porter_infer::{InferReply, InferRequest, ModelError, StopReason};
use prov::{Labelled, Quarantined, ReaderKey, SessionId};
use std::collections::BTreeSet;

/// What the model must be able to do and how much it must hold: chat with structured output, and
/// room for the inputs (three bytes a token) and a page of reply.
fn need_of(inputs: &[Labelled<String>]) -> Need {
    Need::Llm(LlmNeed {
        features: BTreeSet::from([LlmFeature::Chat, LlmFeature::StructuredOutput]),
        context: Tokens(
            u32::try_from(inputs.iter().map(|i| i.value.len()).sum::<usize>() / 3)
                .unwrap_or(u32::MAX)
                .saturating_add(1024),
        ),
    })
}

/// Asks the reader model through `transport` and reads the reply under `ask.want`. A schema with
/// no shape in the structured-output vocabulary is refused before any model is asked.
pub async fn read<T: Transport>(
    transport: &T,
    ask: &ReaderAsk,
    inputs: &[Labelled<String>],
) -> Result<Value, ReaderError> {
    ask.want.shape().map_err(ReaderError::OutOfSchema)?;
    let request = reader_request(ask, inputs);
    let mut session = transport
        .open(&need_of(inputs), request.class, request.tier)
        .await
        .map_err(|_| ReaderError::ModelUnavailable)?;
    // `turn` reads before it believes a failed write: a daemon that refuses a session hangs up
    // after writing the refusal.
    let reply = turn(&mut session, InferRequest::Chat(request), &mut Discard)
        .await
        .map_err(|_| ReaderError::ModelUnavailable)?;
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

/// The reader inside the app's own process: a second model session with no tools, which sees the
/// quarantined text and answers a typed value only. The isolation of the desktop's `readerd` (a
/// separate process) is replaced by the session boundary: the reader's request is built from a
/// fixed instruction and fenced data, never from the planner's words, and the planner never
/// reads the answer's text except as a handle.
#[derive(Debug)]
pub struct TransportReader<T: Transport> {
    key: ReaderKey,
    transport: T,
}

impl<T: Transport> TransportReader<T> {
    /// A reader that asks through `transport`. The key to open quarantined text is made here, in
    /// the one place of this crate that reads: a planner's crates never construct one.
    pub fn in_process(transport: T) -> Self {
        Self {
            key: ReaderKey::for_reader_host(),
            transport,
        }
    }
}

impl<T: Transport> Reader for TransportReader<T> {
    async fn extract(
        &self,
        _session: &SessionId,
        ask: ReaderAsk,
        inputs: Vec<Quarantined<String>>,
    ) -> Result<Value, ReaderError> {
        let opened: Vec<Labelled<String>> = inputs.into_iter().map(|q| q.open(&self.key)).collect();
        read(&self.transport, &ask, &opened).await
    }
}
