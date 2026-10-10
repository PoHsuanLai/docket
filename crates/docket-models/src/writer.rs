//! The policy writer over any porter-client Transport: it reads the person's own turns and the
//! action catalogue, never content, and asks a model for a draft of the task's policy as JSON
//! under a schema; `draft::policy_of` checks the draft. The router stamps `from` and `expires`
//! and bounds the result by the grants (`Router::bound_policy`); if the writer fails there is no
//! policy and every non-read call is outside.

use crate::draft::{Draft, key, policy_of, schema};
use crate::model::{Discard, chat_of, turn};
use docket_core::{ActionCard, Derived, PolicyWriter, ReviewError, UserTurn};
use porter_client::Transport;
use porter_core::capability::LlmFeature;
use porter_core::consent::Usage;
use porter_core::need::LlmNeed;
use porter_core::{DataClass, Need, Permille, Tier, Tokens};
use porter_infer::{
    ChatControl, ChatMessage, ChatRequest, InferRequest, Knob, MessagePart, ModelError, Reasoning,
    ReplyShape, Role, Sampling, ToolChoice, ToolParallelism,
};
use prov::{SpaceId, TaskId};
use std::collections::BTreeSet;

/// The room the reply is counted to need when a model is chosen: the draft and some thought ahead
/// of it. The reply's own limit is the model's (`Knob::Off`).
const REPLY_ROOM: u32 = 2048;

/// The words the model is given. The turns are the person's own; nothing else is in the prompt.
const INSTRUCTION: &str = "You write a task policy: the least an assistant needs to do what \
the person asked, and no more. You are given the person's own words and the list of actions \
that exist. Choose only listed actions. Choose a ceiling no higher than the chosen actions \
need, and no lower: it must cover the effect of every chosen action. Name a recipient, destination or path only if the person wrote it; when the person limits the files to a folder, give that folder in paths. When the person \
asked only to look, choose reading actions only.";

/// The policy writer: reads the person's turns and the action catalogue, never content.
#[derive(Debug)]
pub struct TransportWriter<T: Transport> {
    transport: T,
}

impl<T: Transport> TransportWriter<T> {
    /// Asks through `transport`.
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
}

/// The prompt: the catalogue (names, labels and effects, all the apps' own words) and the
/// person's turns, numbered.
fn prompt(turns: &[UserTurn], catalogue: &[ActionCard]) -> String {
    let actions: String = catalogue
        .iter()
        .map(|c| format!("- {} ({:?}): {}\n", key(c), c.effect, c.label))
        .collect();
    let said: String = turns
        .iter()
        .map(|t| format!("[{}] {}\n", t.id.0, t.text))
        .collect();
    format!("Actions that exist:\n{actions}\nWhat the person said:\n{said}")
}

pub(crate) fn request(turns: &[UserTurn], catalogue: &[ActionCard]) -> ChatRequest {
    let message = |role, text| ChatMessage {
        role,
        parts: vec![MessagePart::Text(text)],
    };
    ChatRequest::new(
        vec![
            message(Role::System, INSTRUCTION.to_owned()),
            message(Role::User, prompt(turns, catalogue)),
        ],
        Tier::Fast,
        // The person's own words: their floor is this computer.
        DataClass::Prompt,
        Usage::Interactive,
    )
    .with_shape(ReplyShape::Json(schema(catalogue)))
    .with_control(
        // No `max_output`: the model's own limit, from its catalogue entry. A model that must
        // reason before it answers spends part of it on thought, which a fixed few hundred
        // tokens cannot hold.
        ChatControl::new()
            .with_tool_choice(ToolChoice::Never)
            .with_tool_calls(ToolParallelism::One)
            .with_max_output(Knob::Off)
            .with_reasoning(Reasoning::Off)
            .with_sampling(Knob::Set(Sampling {
                temperature: Permille(0),
                top_p: Knob::Off,
                top_k: Knob::Off,
                min_p: Knob::Off,
                seed: Knob::Off,
            })),
    )
}

fn need(request: &ChatRequest) -> Need {
    Need::Llm(LlmNeed::new(
        BTreeSet::from([LlmFeature::Chat, LlmFeature::StructuredOutput]),
        Tokens(
            u32::try_from(request.messages.iter().map(message_len).sum::<usize>() / 3)
                .unwrap_or(u32::MAX)
                .saturating_add(REPLY_ROOM),
        ),
    ))
}

fn message_len(message: &ChatMessage) -> usize {
    message
        .parts
        .iter()
        .map(|p| match p {
            MessagePart::Text(t) => t.len(),
            _ => 0,
        })
        .sum()
}

fn failed(error: ModelError) -> ReviewError {
    match error {
        ModelError::OnlyThought { .. } => ReviewError::OnlyThought,
        ModelError::Unparseable | ModelError::Unreadable => ReviewError::Unparseable,
        // Unreachable, rate limited, unauthorized, payment required (top up or pick another
        // model), a refused sign-in, refused, not ready, context overflow, and any error a later
        // porter adds: the reviewer is out of reach, retrying cannot help, and nothing is
        // allowed because of it.
        _ => ReviewError::Unavailable,
    }
}

impl<T: Transport> PolicyWriter for TransportWriter<T> {
    async fn derive(
        &self,
        task: &TaskId,
        turns: &[UserTurn],
        catalogue: &[ActionCard],
        space: &SpaceId,
    ) -> Result<Derived, ReviewError> {
        let request = request(turns, catalogue);
        let mut session = self
            .transport
            .open(&need(&request), request.class, request.tier)
            .await
            .map_err(|_| ReviewError::Unavailable)?;
        let reply = turn(&mut session, InferRequest::Chat(request), &mut Discard)
            .await
            .map_err(failed)?;
        let chat = chat_of(reply).map_err(failed)?;
        match chat.stop {
            porter_infer::StopReason::EndTurn | porter_infer::StopReason::StopSequence => {}
            porter_infer::StopReason::MaxTokens => return Err(ReviewError::OutOfRoom),
            _ => return Err(ReviewError::Unparseable),
        }
        let draft: Draft =
            serde_json::from_str(&chat.text).map_err(|_| ReviewError::Unparseable)?;
        Ok(policy_of(draft, task, turns, catalogue, space))
    }
}
