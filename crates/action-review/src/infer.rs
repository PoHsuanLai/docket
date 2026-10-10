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

/// What the model has put out so far, as the events told it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Heard {
    /// Nothing yet.
    Nothing,
    /// Reasoning and no reply text: the budget may have gone on thinking.
    OnlyThought,
    /// Reply text.
    Text,
}

/// Keeps what kind of output went by, and drops the rest: the reviewer reads the finished reply.
struct Watch(Heard);

impl ChatSink for Watch {
    fn event(&mut self, event: InferEvent) -> Flow {
        self.0 = match (event, self.0) {
            (InferEvent::TextDelta(text), _) if !text.is_empty() => Heard::Text,
            (InferEvent::ThoughtDelta(_), Heard::Nothing) => Heard::OnlyThought,
            (_, heard) => heard,
        };
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

/// The Quick judge's option that means "ask".
pub(crate) const FLAG: &str = "flag";

/// The Quick judge's declared options, in the order it is shown them.
pub(crate) fn quick_options() -> Vec<String> {
    vec!["pass".to_owned(), FLAG.to_owned()]
}

fn shape(stage: Stage) -> ReplyShape {
    match stage {
        Stage::Quick => ReplyShape::Choice(quick_options()),
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

/// A reviewer answers the same way every time: temperature zero, no tools, no reasoning at all
/// (a thinking model spends a short budget on thought and writes no verdict; the record already
/// carries a `reason`), and a short reply.
fn control(stage: Stage) -> ChatControl {
    let max_output = match stage {
        Stage::Quick => Tokens(8),
        Stage::Deliberate | Stage::SecondOpinion => Tokens(320),
    };
    ChatControl {
        tool_choice: ToolChoice::Never,
        tool_calls: ToolParallelism::One,
        max_output: Knob::Set(max_output),
        reasoning: Reasoning::Off,
        sampling: Knob::Set(Sampling {
            temperature: Permille(0),
            top_p: Knob::Off,
            top_k: Knob::Off,
            min_p: Knob::Off,
            seed: Knob::Off,
        }),
        stop: Vec::new(),
        scores: Knob::Off,
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

fn model_failed(error: ModelError, heard: Heard) -> ReviewError {
    match error {
        ModelError::OnlyThought { .. } => ReviewError::OnlyThought,
        ModelError::Unparseable | ModelError::Unreadable if heard == Heard::OnlyThought => {
            ReviewError::OnlyThought
        }
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
    let only_thought = reply.text.trim().is_empty()
        && reply.tool_calls.is_empty()
        && reply
            .thought
            .as_deref()
            .is_some_and(|t| !t.trim().is_empty());
    if only_thought {
        return Err(ReviewError::OnlyThought);
    }
    match reply.stop {
        StopReason::EndTurn | StopReason::StopSequence => Ok(reply.text),
        // Out of room is a limit too small, not a model that refused; it fails closed all the
        // same (the person is asked), and the audit says which it was.
        StopReason::MaxTokens => Err(ReviewError::OutOfRoom),
        StopReason::ContentFilter | StopReason::ToolUse => Err(ReviewError::Unparseable),
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
        let mut watch = Watch(Heard::Nothing);
        let reply = model
            .chat(&chat_request(stage, request), &mut watch)
            .await
            .map_err(|error| model_failed(error, watch.0))?;
        parse_verdict(&complete(reply)?, stage)
    }
}
