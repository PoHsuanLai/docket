//! One chat exchange as a [`ModelExchange`]: the request begun, the route noted, the reply
//! finished. Closed sets are written by their serde slug, so a change to a `Debug` impl cannot
//! change a trace.

use docket_core::{ExchangeAnswer, ExchangeCall, ExchangeMessage, ModelExchange};
use porter_infer::{ChatRequest, InferEvent, InferReply, MessagePart, ReplyShape, Role};
use serde::Serialize;

/// The slug of a closed set, from its serde form.
fn slug<V: Serialize>(value: &V) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// The words of one part of a message.
fn words(part: &MessagePart) -> Option<String> {
    match part {
        MessagePart::Text(text) => Some(text.clone()),
        MessagePart::Image(_) => Some("[image]".to_owned()),
        MessagePart::ToolCall(call) => {
            Some(format!("[call {} {}]", slug(&call.name), slug(&call.args)))
        }
        MessagePart::ToolResult(result) => {
            let inner: Vec<String> = result.parts.iter().filter_map(words).collect();
            Some(format!("[result {:?}: {}]", result.status, inner.join(" ")))
        }
        MessagePart::Thought(_) => None,
        _ => Some("[part]".to_owned()),
    }
}

fn role(role: Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
    }
}

fn shape(shape: &ReplyShape) -> &'static str {
    match shape {
        ReplyShape::Text => "text",
        ReplyShape::Json(_) => "json",
        ReplyShape::Choice(_) => "choice",
        _ => "other",
    }
}

/// An exchange begun: everything but the answer.
pub(crate) fn begin(by: &str, n: u32, request: &ChatRequest) -> ModelExchange {
    ModelExchange {
        by: by.to_owned(),
        n,
        tier: slug(&request.tier),
        class: slug(&request.class),
        shape: shape(&request.shape).to_owned(),
        tools: request.tools.iter().map(|t| slug(&t.name)).collect(),
        messages: request
            .messages
            .iter()
            .map(|m| ExchangeMessage {
                role: role(m.role).to_owned(),
                text: m
                    .parts
                    .iter()
                    .filter_map(words)
                    .collect::<Vec<_>>()
                    .join("\n"),
            })
            .collect(),
        route: Vec::new(),
        answer: ExchangeAnswer::Cancelled,
        took_ms: 0,
        input_tokens: 0,
        output_tokens: 0,
    }
}

/// What inferd said about the route, in one line, if the event is such a note.
pub(crate) fn route_note(event: &InferEvent) -> Option<String> {
    match event {
        InferEvent::Routed(served) => Some(format!(
            "routed to {}/{} ({})",
            slug(&served.account),
            slug(&served.model),
            slug(&served.locality)
        )),
        InferEvent::Why(why) => Some(format!("why: {}", slug(why))),
        InferEvent::Declined(declined) => Some(format!("declined: {declined:?}")),
        _ => None,
    }
}

/// The exchange finished by `reply`.
pub(crate) fn finish(mut open: ModelExchange, reply: &InferReply, took_ms: u32) -> ModelExchange {
    open.took_ms = took_ms;
    open.answer = match reply {
        InferReply::Chat(chat) => {
            open.input_tokens = chat.usage.input.0;
            open.output_tokens = chat.usage.output.0;
            ExchangeAnswer::Replied {
                text: chat.text.clone(),
                calls: chat
                    .tool_calls
                    .iter()
                    .map(|c| ExchangeCall {
                        name: slug(&c.name),
                        args: slug(&c.args),
                    })
                    .collect(),
                stop: slug(&chat.stop),
            }
        }
        InferReply::Refused(why) => ExchangeAnswer::Refused(format!("{why:?}")),
        InferReply::Failed(why) => ExchangeAnswer::Failed(format!("{why:?}")),
        InferReply::Cancelled => ExchangeAnswer::Cancelled,
        other => ExchangeAnswer::Failed(format!("unexpected reply {other:?}")),
    };
    open
}
