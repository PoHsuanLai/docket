//! What an agent tells us it is doing, as opposed to what it asks us to do. A `tool_call` it
//! reports is a record, `acp.<program>.reported.<kind>`, and it is never dispatched, never
//! trusted, and never a reason to let anything through: the agent may claim it ran anything. Its
//! message text is its words (untrusted); its thoughts are `Thought` events and nothing reads
//! them as instructions or as the person's.

use super::gate::Audit;
use super::names;
use agent_client_protocol_schema::v1::{
    ContentBlock, ContentChunk, SessionUpdate, ToolCallStatus, ToolKind,
};
use docket_core::{
    AppRefusal, CallId, CallRefusal, FailText, Reveal, StepEnd, StepLine, StepShown,
};
use docket_session::{BackendEvent, CallEvent, CallOpen, ProgramName, UsageNote};
use porter_core::{Count, MicroUsd};
use prov::Effect;
use std::collections::BTreeMap;

/// What was heard.
#[derive(Debug, Default)]
pub struct Heard {
    /// Events to hand on, in order.
    pub events: Vec<BackendEvent>,
    /// The agent's own tool brought untrusted content into the session.
    pub taint: bool,
    /// Lines for the audit.
    pub audit: Vec<Audit>,
}

#[derive(Debug, Clone)]
struct Open {
    call: CallId,
    action: docket_core::ActionRef,
    effect: Effect,
}

/// The agent's own tool calls that have begun and not ended.
#[derive(Debug, Default)]
pub struct Reported {
    open: BTreeMap<String, Open>,
}

fn text(chunk: &ContentChunk) -> Option<String> {
    match &chunk.content {
        ContentBlock::Text(t) => Some(t.text.clone()),
        _ => None,
    }
}

fn brings_content(kind: ToolKind) -> bool {
    matches!(
        kind,
        ToolKind::Read | ToolKind::Search | ToolKind::Fetch | ToolKind::Execute | ToolKind::Other
    )
}

fn end_of(open: &Open, end: StepEnd) -> BackendEvent {
    BackendEvent::Call(CallEvent::Ended(StepLine {
        call: open.call,
        action: open.action.clone(),
        effect: open.effect,
        end,
        shown: StepShown::Full,
        with: Vec::new(),
    }))
}

fn done() -> StepEnd {
    StepEnd::Done {
        said: None,
        value: None,
        undo: None,
    }
}

fn failed() -> StepEnd {
    StepEnd::Refused(CallRefusal::App(AppRefusal::Failed(FailText(
        "the agent reported that the call failed".to_owned(),
    ))))
}

fn usage(update: &agent_client_protocol_schema::v1::UsageUpdate) -> UsageNote {
    let spent = update
        .cost
        .as_ref()
        .filter(|c| c.currency == "USD" && c.amount.is_finite() && c.amount >= 0.0)
        .map(|c| MicroUsd((c.amount * 1_000_000.0) as u64));
    UsageNote {
        context: Some(Count(u32::try_from(update.used).unwrap_or(u32::MAX))),
        spent,
    }
}

impl Reported {
    /// Hears one `session/update`. `next` is the connection's call counter.
    pub fn hear(&mut self, program: &ProgramName, next: &mut u64, update: SessionUpdate) -> Heard {
        let mut heard = Heard::default();
        match update {
            SessionUpdate::AgentMessageChunk(chunk) => heard
                .events
                .extend(text(&chunk).map(|t| BackendEvent::Words(Reveal::Plain(t)))),
            SessionUpdate::AgentThoughtChunk(chunk) => {
                heard.events.extend(text(&chunk).map(BackendEvent::Thought))
            }
            SessionUpdate::UsageUpdate(u) => heard.events.push(BackendEvent::Usage(usage(&u))),
            SessionUpdate::ToolCall(call) => {
                let key = call.tool_call_id.0.to_string();
                let Some(action) = names::action(program, &names::reported_tail(call.kind)) else {
                    return heard;
                };
                *next += 1;
                let open = Open {
                    call: CallId(*next),
                    action,
                    effect: names::effect(call.kind),
                };
                heard.taint = brings_content(call.kind);
                heard.audit.push(Audit::Reported {
                    action: open.action.name.as_str().to_owned(),
                });
                heard
                    .events
                    .push(BackendEvent::Call(CallEvent::Started(CallOpen {
                        call: open.call,
                        action: open.action.clone(),
                        effect: open.effect,
                    })));
                match call.status {
                    ToolCallStatus::Completed => heard.events.push(end_of(&open, done())),
                    ToolCallStatus::Failed => heard.events.push(end_of(&open, failed())),
                    _ => {
                        self.open.insert(key, open);
                    }
                }
            }
            SessionUpdate::ToolCallUpdate(update) => {
                let key = update.tool_call_id.0.to_string();
                let end = match update.fields.status {
                    Some(ToolCallStatus::Completed) => done(),
                    Some(ToolCallStatus::Failed) => failed(),
                    _ => return heard,
                };
                if let Some(open) = self.open.remove(&key) {
                    heard.events.push(end_of(&open, end));
                }
            }
            _ => {}
        }
        heard
    }

    /// The turn is over: whatever the agent began and never ended ends interrupted.
    pub fn close_turn(&mut self) -> Vec<BackendEvent> {
        std::mem::take(&mut self.open)
            .into_values()
            .map(|open| end_of(&open, StepEnd::Interrupted))
            .collect()
    }
}
