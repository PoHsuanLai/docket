//! What a run trace keeps of one model exchange: the words of the request as the caller sent
//! them, how inferd routed it, the answer, and how long it took. Plain data: the tap in
//! `docket-dbus` writes it, `docket-eval` renders it and turns it into a cassette. It holds
//! prompts, so it is written only where a live run asks for it (never by default, never to a
//! shared place).

use serde::{Deserialize, Serialize};

/// One message of the request, in the words the model saw.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeMessage {
    /// `system`, `user` or `assistant`.
    pub role: String,
    /// The text; a tool call or result in the conversation is a bracketed line.
    pub text: String,
}

/// One function call the model made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeCall {
    /// The function's name on the wire.
    pub name: String,
    /// Its arguments, as the JSON text the model wrote.
    pub args: String,
}

/// How the exchange ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ExchangeAnswer {
    /// The model answered: words, calls, and why it stopped.
    Replied {
        /// The text (JSON text for a structured reply).
        text: String,
        /// The calls, in order.
        calls: Vec<ExchangeCall>,
        /// The stop reason, as its slug.
        stop: String,
    },
    /// inferd refused before any model ran.
    Refused(String),
    /// The model call failed.
    Failed(String),
    /// The caller hung up first.
    Cancelled,
}

/// One chat request and its answer, as the daemon that asked saw them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelExchange {
    /// Which process asked (`intentd`, `companiond`, `readerd`, or the eval runner).
    pub by: String,
    /// Its place in that tap's order, from 1.
    pub n: u32,
    /// The tier asked for.
    pub tier: String,
    /// The data class the request carries.
    pub class: String,
    /// The reply shape: `text`, `json` or `choice`.
    pub shape: String,
    /// The tools offered, by name.
    pub tools: Vec<String>,
    /// The conversation sent.
    pub messages: Vec<ExchangeMessage>,
    /// What inferd said about the route (who answers, why), in order.
    pub route: Vec<String>,
    /// The answer.
    pub answer: ExchangeAnswer,
    /// Milliseconds from the request to the end of the turn.
    pub took_ms: u32,
    /// Prompt tokens spent.
    pub input_tokens: u32,
    /// Reply tokens spent.
    pub output_tokens: u32,
}

impl ModelExchange {
    /// Every word the model was sent, joined: what a `lacks` or `contains` check reads.
    pub fn sent_text(&self) -> String {
        self.messages
            .iter()
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    }
}
