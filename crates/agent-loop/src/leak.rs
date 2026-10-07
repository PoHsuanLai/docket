//! A tool call a model left in its words. Some servers' tool parsers fail (an unknown template,
//! a half-written call) and hand back the model's raw call as the reply's text. The text is
//! never a call: nothing here turns it into one. What this reads is only whether the words are
//! a call that went astray, so the planner can end the turn with a typed failure instead of
//! showing the person a pile of markup as the answer.
//!
//! Matching is forgiving about the dress (case, zero-width and bidirectional marks, fullwidth
//! angle brackets) because a hostile or confused model controls the bytes; it is exact about
//! the markers, which are the ones the common chat templates write.

use serde::{Deserialize, Serialize};

/// Which template's call it looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LeakForm {
    /// Hermes: `<tool_call>{"name": ..., "arguments": {...}}</tool_call>`.
    Hermes,
    /// Qwen3-coder: `<tool_call><function=name><parameter=key>value</parameter></function></tool_call>`.
    QwenXml,
    /// Mistral: `[TOOL_CALLS] [...]`.
    Mistral,
    /// Llama: `<|python_tag|>` followed by a call.
    LlamaTag,
    /// Any other tag a template names a call by.
    OtherTag,
}

/// Words that are a call which was never made.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeakedCall {
    /// The template it looks like.
    pub form: LeakForm,
}

/// Marks that draw nothing or change the direction of text: dropped before matching.
fn invisible(c: char) -> bool {
    c.is_control()
        || docket_core::reorders(c)
        || docket_core::hides(c)
        || matches!(c, '\u{200C}'..='\u{200F}')
}

/// The text as a matcher reads it: lower case, no invisible marks, ASCII angle brackets and
/// pipes where fullwidth ones were written.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !invisible(*c) || c.is_whitespace())
        .map(|c| match c {
            '\u{FF1C}' | '\u{2039}' | '\u{3008}' => '<',
            '\u{FF1E}' | '\u{203A}' | '\u{3009}' => '>',
            '\u{FF5C}' => '|',
            other => other,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether `text` is, or holds, a tool call the model wrote instead of making. `None` for
/// ordinary words, including words that talk about tools.
pub fn leaked_call(text: &str) -> Option<LeakedCall> {
    let plain = plain(text);
    let form = if plain.contains("<function=") {
        LeakForm::QwenXml
    } else if plain.contains("<tool_call>") || plain.contains("<tool_call ") {
        LeakForm::Hermes
    } else if plain.contains("[tool_calls]") {
        LeakForm::Mistral
    } else if plain.contains("<|python_tag|>") {
        LeakForm::LlamaTag
    } else if [
        "<function_call>",
        "<|tool_call|>",
        "<|tool_call_begin|>",
        "<tool_use>",
    ]
    .iter()
    .any(|tag| plain.contains(tag))
    {
        LeakForm::OtherTag
    } else {
        return None;
    };
    Some(LeakedCall { form })
}
