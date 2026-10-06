//! Turning a live run into a cassette: the model exchanges the tap kept, in order, become the
//! entries of inferd's replay cassette (JSON Lines: a header, then one entry per exchange), so a
//! failure seen against a real model replays deterministically in the gate.
//!
//! An entry says what the request must look like (tools offered or not, and an anchor from the
//! first message so a reviewer's prompt and a writer's cannot answer each other) and what the
//! model said. Entries play once and in order; a replayed case that asks something the live run
//! did not ask finds no entry and ends Failed, which is the point: the case no longer
//! reproduces. The anchor is the head of the system message, never content the person wrote.

use docket_core::{ExchangeAnswer, ModelExchange};
use serde_json::{Value, json};

/// The anchor: the first words of the first system message, kept short.
const ANCHOR_CHARS: usize = 48;

/// The header line stoker's cassette format starts with.
fn header(build: &str) -> Value {
    json!({
        "vocab": 1,
        "engine": { "kind": "replay", "build": build },
        "model": "scripted",
        "recorded": 0,
        "context": { "loaded": 32768, "trained": 32768 },
        "speech": null
    })
}

fn anchor(exchange: &ModelExchange) -> Vec<String> {
    exchange
        .messages
        .iter()
        .find(|m| m.role == "system")
        .map(|m| m.text.chars().take(ANCHOR_CHARS).collect::<String>())
        .filter(|a| !a.trim().is_empty())
        .into_iter()
        .collect()
}

/// What the engine said for words the caller read: a choice comes back to the caller as the
/// bare word, but the engine renders it as a JSON string, which is what a cassette must hold.
fn engine_text(shape: &str, text: &str) -> String {
    match shape {
        "choice" => serde_json::to_string(text).unwrap_or_default(),
        _ => text.to_owned(),
    }
}

fn reply(shape: &str, answer: &ExchangeAnswer) -> Value {
    match answer {
        ExchangeAnswer::Replied { calls, .. } if !calls.is_empty() => json!({
            "kind": "calls",
            "v": calls.iter().map(|c| json!({
                "name": c.name,
                "arguments": serde_json::from_str::<Value>(&c.args)
                    .unwrap_or_else(|_| Value::String(c.args.clone())),
            })).collect::<Vec<_>>()
        }),
        ExchangeAnswer::Replied { text, .. } => {
            json!({ "kind": "text", "v": engine_text(shape, text) })
        }
        ExchangeAnswer::Refused(_) | ExchangeAnswer::Failed(_) | ExchangeAnswer::Cancelled => {
            json!({ "kind": "fail", "v": 503 })
        }
    }
}

fn entry(exchange: &ModelExchange) -> Value {
    json!({
        "when": {
            "tools": if exchange.tools.is_empty() { "absent" } else { "present" },
            "contains": anchor(exchange),
        },
        "reply": reply(&exchange.shape, &exchange.answer),
    })
}

/// The cassette that replays `exchanges`, as the text of a `.jsonl` file. `build` names what
/// made it (it lands in the header).
pub fn cassette_from(exchanges: &[ModelExchange], build: &str) -> String {
    std::iter::once(header(build))
        .chain(exchanges.iter().map(entry))
        .map(|line| format!("{line}\n"))
        .collect()
}
