//! The model-backed reviewer: one model per stage over porter-infer's `Model` seam. A failure of
//! any kind is an error, which `tighten` turns into a confirmation; the reviewer never allows
//! by failing.
//!
//! The stage timeout is `ReviewTimeouts`: a stage with no time at all (`0 ms`) fails before
//! asking. A running deadline needs a timer, which a pure crate does not own: the router races
//! every `review` call against its clock's `after`.

use crate::cascade::Reviewer;
use crate::parse::parse_verdict;
use crate::render::render;
use crate::request::{CODES, ReviewRequest, slug};
use crate::verdict::ReviewVerdict;
use docket_core::{Millis, ReviewError, ReviewTimeouts, Stage};
use porter_core::consent::Usage;
use porter_core::{DataClass, Permille, Tier, Tokens};
use porter_infer::{
    ChatControl, ChatMessage, ChatReply, ChatRequest, ChatSink, Flow, InferEvent, Knob,
    MessagePart, Model, ModelError, Reasoning, ReplyShape, Role, Sampling, StopReason, ToolChoice,
    ToolParallelism,
};
use serde_json::json;

/// Reviews with three models, one per stage.
#[derive(Debug)]
pub struct InferReviewer<M> {
    /// The quick judge.
    pub quick: M,
    /// The deliberate model.
    pub deliberate: M,
    /// The second opinion.
    pub second: M,
    /// The stage timeouts.
    pub timeouts: ReviewTimeouts,
}

/// Drops every event: the reviewer reads the finished reply.
struct Discard;

impl ChatSink for Discard {
    fn event(&mut self, _event: InferEvent) -> Flow {
        Flow::Continue
    }
}

/// The JSON Schema of the record the larger stages reply with. Hand-written until stoker's
/// `Shape::to_json_schema` is filled (then `verdict_shape` replaces it).
fn record_schema() -> String {
    let codes: Vec<String> = CODES.iter().map(slug).collect();
    json!({
        "type": "object",
        "properties": {
            "verdict": { "type": "string", "enum": ["allow", "ask", "deny"] },
            "code": { "type": "string", "enum": codes },
            "reason": { "type": "string", "maxLength": 200 }
        },
        "required": ["verdict", "code", "reason"],
        "additionalProperties": false
    })
    .to_string()
}

fn shape(stage: Stage) -> ReplyShape {
    match stage {
        Stage::Quick => ReplyShape::Choice(vec!["pass".to_owned(), "flag".to_owned()]),
        Stage::Deliberate | Stage::SecondOpinion => ReplyShape::Json(record_schema()),
    }
}

/// The tier a stage asks for: the user maps tiers to models.
fn tier(stage: Stage) -> Tier {
    match stage {
        Stage::Quick => Tier::Fast,
        Stage::Deliberate => Tier::Balanced,
        Stage::SecondOpinion => Tier::Best,
    }
}

/// A reviewer answers the same way every time: temperature zero, no tools, no reasoning for the
/// one-token judge, and a short reply.
fn control(stage: Stage) -> ChatControl {
    let (max_output, reasoning) = match stage {
        Stage::Quick => (Tokens(8), Reasoning::Off),
        Stage::Deliberate | Stage::SecondOpinion => (Tokens(320), Reasoning::EngineDefault),
    };
    ChatControl {
        tool_choice: ToolChoice::Never,
        tool_calls: ToolParallelism::One,
        max_output: Knob::Set(max_output),
        reasoning,
        sampling: Knob::Set(Sampling {
            temperature: Permille(0),
            top_p: Knob::Off,
            top_k: Knob::Off,
            min_p: Knob::Off,
            seed: Knob::Off,
        }),
        stop: Vec::new(),
    }
}

fn message(role: Role, text: String) -> ChatMessage {
    ChatMessage {
        role,
        parts: vec![MessagePart::Text(text)],
    }
}

/// The data class every reviewer request carries: `Prompt`, the person's own words and what a
/// reviewer quotes of them. Under the proposed AI policy its floor is this computer
/// (`ai.floor.prompt`), so a reviewer request is pinned on-device whatever the models are; a
/// cloud model would be refused by inferd (`RequiresCloud`), never sent the words.
pub const REVIEW_CLASS: DataClass = DataClass::Prompt;

/// The chat request for one stage of one review.
pub(crate) fn chat_request(stage: Stage, request: &ReviewRequest) -> ChatRequest {
    let prompt = render(request, stage);
    ChatRequest {
        messages: vec![
            message(Role::System, prompt.system),
            message(Role::User, prompt.user),
        ],
        shape: shape(stage),
        tier: tier(stage),
        class: REVIEW_CLASS,
        usage: Usage::Interactive,
        tools: Vec::new(),
        control: control(stage),
    }
}

fn model_failed(error: ModelError) -> ReviewError {
    match error {
        ModelError::Unparseable | ModelError::Unreadable => ReviewError::Unparseable,
        ModelError::Unreachable
        | ModelError::RateLimited(_)
        | ModelError::Unauthorized
        | ModelError::Refused
        | ModelError::NotReady
        | ModelError::ContextOverflow => ReviewError::Unavailable,
    }
}

/// A reply cut short by its limit or a filter is not a verdict, whatever its text says.
fn complete(reply: ChatReply) -> Result<String, ReviewError> {
    match reply.stop {
        StopReason::EndTurn | StopReason::StopSequence => Ok(reply.text),
        StopReason::MaxTokens | StopReason::ContentFilter | StopReason::ToolUse => {
            Err(ReviewError::Unparseable)
        }
    }
}

impl<M: Model> InferReviewer<M> {
    fn stage(&self, stage: Stage) -> (&M, Millis) {
        match stage {
            Stage::Quick => (&self.quick, self.timeouts.quick),
            Stage::Deliberate => (&self.deliberate, self.timeouts.deliberate),
            Stage::SecondOpinion => (&self.second, self.timeouts.second),
        }
    }
}

impl<M: Model> Reviewer for InferReviewer<M> {
    async fn review(
        &self,
        stage: Stage,
        request: &ReviewRequest,
    ) -> Result<ReviewVerdict, ReviewError> {
        let (model, allowed) = self.stage(stage);
        if allowed == Millis(0) {
            return Err(ReviewError::Timeout);
        }
        let reply = model
            .chat(&chat_request(stage, request), &mut Discard)
            .await
            .map_err(model_failed)?;
        parse_verdict(&complete(reply)?, stage)
    }
}
