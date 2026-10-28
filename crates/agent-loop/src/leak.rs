//! A tool call a model left in its words. Some servers' tool parsers fail (an unknown template,
//! a half-written call) and hand back the model's raw call as the reply's text. The text is
//! never a call: nothing here turns it into one. What this reads is only whether the words are
//! a call that went astray, so the planner can end the turn with a typed failure instead of
//! showing the person a pile of markup as the answer.
//!
//! Matching is forgiving about the dress (case, zero-width and bidirectional marks, fullwidth
//! angle brackets) because a hostile or confused model controls the bytes; it is exact about
//! the markers, which are the ones the common chat templates write.

/// The markers the common chat templates write around a call, in lower case: Qwen3-coder's
/// `<function=name>`, Hermes's `<tool_call>`, Mistral's `[TOOL_CALLS]`, Llama's `<|python_tag|>`
/// and the other tags a template names a call by. Which template wrote it is not told apart.
const TAGS: [&str; 9] = [
    "<function=",
    "<tool_call>",
    "<tool_call ",
    "[tool_calls]",
    "<|python_tag|>",
    "<function_call>",
    "<|tool_call|>",
    "<|tool_call_begin|>",
    "<tool_use>",
];

/// Marks that draw nothing or change the direction of text: dropped before matching.
fn invisible(c: char) -> bool {
    c.is_control()
        || docket_vocab::reorders(c)
        || docket_vocab::hides(c)
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

/// The marker `text` holds when it is, or holds, a tool call the model wrote instead of making.
/// `None` for ordinary words, including words that talk about tools.
pub fn leaked_call(text: &str) -> Option<&'static str> {
    let plain = plain(text);
    TAGS.into_iter().find(|tag| plain.contains(tag))
}
