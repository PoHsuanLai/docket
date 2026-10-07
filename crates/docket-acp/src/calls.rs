//! Our step lines as ACP tool calls. A refused or unconfirmed call is `failed` with one coarse
//! sentence (the planner sees only a `DenyCode`; so does the editor). `rawInput` and `rawOutput`
//! are never set: what a call returned may hold handles and private Space data the editor was not
//! granted, so only the app's own `said` text goes out.

use agent_client_protocol_schema::v1::{
    Content, ContentBlock, ContentChunk, SessionUpdate, ToolCall, ToolCallContent, ToolCallId,
    ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use docket_core::{CallRefusal, DenyCode, StepEnd, StepLine};
use docket_session::CallOpen;
use prov::Effect;

/// How a call ended, for the editor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It ran; the app's own words, if it said any.
    Completed(Option<String>),
    /// It did not run, or did not finish; one coarse sentence.
    Failed(&'static str),
}

fn denied(code: DenyCode) -> &'static str {
    match code {
        DenyCode::OutsideTask => "Outside what you asked for.",
        DenyCode::NotAllowed => "Not something the assistant may do.",
        DenyCode::NeedsUser => "Needs your confirmation on the desktop.",
        DenyCode::Repeated => "Already refused.",
    }
}

fn refused(why: &CallRefusal) -> &'static str {
    match why {
        CallRefusal::Denied(code) => denied(*code),
        CallRefusal::App(_) => "The app declined.",
        CallRefusal::Unconfirmed(_) => "Not confirmed.",
        CallRefusal::Halted(_) => "Stopped: the space is halted.",
        CallRefusal::Paused(_) => "Paused until you say something.",
        CallRefusal::OverBudget(_) => "Over its budget.",
        CallRefusal::NoSuchAction(_) => "No such action.",
        CallRefusal::BadArgs { .. } => "The arguments were not accepted.",
        CallRefusal::AppUnavailable(_) => "The app is not available.",
        CallRefusal::Timeout => "The app did not answer in time.",
        CallRefusal::NotRecorded => "It could not be recorded, so it did not run.",
    }
}

/// What the editor is told of how a step ended.
pub fn outcome(end: &StepEnd) -> Outcome {
    match end {
        StepEnd::Done { said, .. } => {
            Outcome::Completed(said.as_ref().map(|s| s.as_str().to_owned()))
        }
        StepEnd::Refused(why) => Outcome::Failed(refused(why)),
        StepEnd::Unconfirmed(_) => Outcome::Failed("Not confirmed on the desktop."),
        StepEnd::Held(_) => Outcome::Failed("Skipped: the same call was made before."),
        StepEnd::Unread(_) => Outcome::Failed("The assistant's request could not be read."),
        StepEnd::Interrupted => Outcome::Failed("Interrupted by a restart; it may have run."),
    }
}

fn kind(open: &CallOpen) -> ToolKind {
    match open.effect {
        Effect::Read if open.action.name.as_str().contains("search") => ToolKind::Search,
        Effect::Read => ToolKind::Read,
        Effect::UndoableWrite => ToolKind::Edit,
        Effect::Outbound => ToolKind::Other,
        Effect::Destructive => ToolKind::Delete,
    }
}

pub(crate) fn call_id(call: docket_core::CallId) -> ToolCallId {
    ToolCallId::new(format!("call-{}", call.0))
}

fn text_content(text: String) -> Vec<ToolCallContent> {
    vec![ToolCallContent::Content(Content::new(ContentBlock::from(
        text,
    )))]
}

/// A call began: `tool_call`, pending.
pub fn started(open: &CallOpen) -> SessionUpdate {
    SessionUpdate::ToolCall(
        ToolCall::new(call_id(open.call), open.action.name.as_str())
            .kind(kind(open))
            .status(ToolCallStatus::Pending),
    )
}

fn update(call: docket_core::CallId, fields: ToolCallUpdateFields) -> SessionUpdate {
    SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(call_id(call), fields))
}

/// The call is going ahead.
pub fn running(open: &CallOpen) -> SessionUpdate {
    update(
        open.call,
        ToolCallUpdateFields::new().status(ToolCallStatus::InProgress),
    )
}

/// The call ended as the router ended it.
pub fn ended(step: &StepLine) -> SessionUpdate {
    let fields = match outcome(&step.end) {
        Outcome::Completed(said) => ToolCallUpdateFields::new()
            .status(ToolCallStatus::Completed)
            .content(said.map(text_content)),
        Outcome::Failed(why) => ToolCallUpdateFields::new()
            .status(ToolCallStatus::Failed)
            .content(text_content(why.to_owned())),
    };
    update(step.call, fields)
}

/// The call was stopped before it ran, by the editor.
pub fn stopped(open: &CallOpen, why: &'static str) -> SessionUpdate {
    update(
        open.call,
        ToolCallUpdateFields::new()
            .status(ToolCallStatus::Failed)
            .content(text_content(why.to_owned())),
    )
}

/// Words from the assistant.
pub fn say(text: impl Into<String>) -> SessionUpdate {
    SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::from(text.into())))
}

/// Words the person said, for a replay.
pub fn said(text: impl Into<String>) -> SessionUpdate {
    SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::from(text.into())))
}
