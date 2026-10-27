//! The tools every task has beside the actions, and the control of the planner's chat requests.

use crate::meta_reply::MOST_WORDS;
use crate::planner::{TOOL_ANSWER, TOOL_ASK, TOOL_FINISH, TOOL_READ, TOOL_RELATED};
use crate::show::MOST_SHOWN;
use crate::want::want_schema;
use docket_core::ActionCard;
use porter_infer::{
    ChatControl, JsonSchemaText, JsonText, Knob, Reasoning, ToolChoice, ToolDecl, ToolName,
    ToolParallelism,
};
use serde_json::{Value as Json, json};

fn schema_text(schema: &Json) -> Option<JsonSchemaText> {
    let text = serde_json::to_string(schema).ok()?;
    Some(JsonSchemaText(JsonText::parse(&text).ok()?))
}

fn meta_tool(name: &str, description: &str, params: &Json) -> Option<ToolDecl> {
    Some(ToolDecl {
        name: ToolName::parse(name).ok()?,
        description: description.to_owned(),
        params: schema_text(params)?,
    })
}

/// The relations the view's actions resolve, each name once, in the view's order.
fn relation_names(actions: &[ActionCard]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for relation in actions.iter().flat_map(|card| &card.related) {
        if !names.iter().any(|n| n == relation.name.as_str()) {
            names.push(relation.name.as_str().to_owned());
        }
    }
    names
}

/// `quire_related`, when some action of the view resolves a relation.
fn related_tool(actions: &[ActionCard]) -> Option<ToolDecl> {
    let names = relation_names(actions);
    if names.is_empty() {
        return None;
    }
    let params = json!({
        "type": "object",
        "properties": {
            "of": {
                "anyOf": [
                    {
                        "type": "object",
                        "properties": { "handle": { "type": "integer", "minimum": 0 } },
                        "required": ["handle"],
                        "additionalProperties": false,
                    },
                    {
                        "type": "object",
                        "properties": {
                            "app": { "type": "string" },
                            "kind": { "type": "string" },
                            "key": { "type": "string" },
                        },
                        "required": ["app", "kind", "key"],
                        "additionalProperties": false,
                    },
                ],
            },
            "relation": { "enum": names },
        },
        "required": ["of", "relation"],
        "additionalProperties": false,
    });
    meta_tool(
        TOOL_RELATED,
        "Get a thing related to a thing you hold, as a handle: \"of\" is the thing, as {\"handle\": n} or as {\"app\", \"kind\", \"key\"}, and \"relation\" is one of the relations shown beside it (\"related: ...\"). Use it when an action needs a thing of another kind that you cannot read from the one you hold, such as who sent a thread.",
        &params,
    )
}

/// The tools every task has beside the actions (`quire_related` first, where it is offered, so
/// the last four are always answer, ask, read and finish).
pub(crate) fn meta_tools(actions: &[ActionCard]) -> Vec<ToolDecl> {
    let answer = json!({
        "type": "object",
        "properties": { "text": { "type": "string", "maxLength": MOST_WORDS } },
        "required": ["text"],
        "additionalProperties": false,
    });
    let ask = json!({
        "type": "object",
        "properties": {
            "text": { "type": "string", "maxLength": 400 },
            "choices": { "type": "array", "items": { "type": "string", "maxLength": 80 }, "maxItems": 6 },
        },
        "required": ["text"],
        "additionalProperties": false,
    });
    let read = json!({
        "type": "object",
        "properties": {
            "inputs": { "type": "array", "items": { "type": "integer", "minimum": 0 }, "minItems": 1 },
            "task": { "enum": ["classify", "extract", "summarise", "compare"] },
            "want": want_schema(),
        },
        "required": ["inputs", "task", "want"],
        "additionalProperties": false,
    });
    let finish = json!({
        "type": "object",
        "properties": {
            "show": { "type": "array", "items": { "type": "integer", "minimum": 0 }, "maxItems": MOST_SHOWN },
        },
        "additionalProperties": false,
    });
    [
        related_tool(actions),
        meta_tool(
            TOOL_ANSWER,
            "Give the person your reply: the answer itself, short, as they would want to read it, with nothing about how you found it. The turn ends with it.",
            &answer,
        ),
        meta_tool(TOOL_ASK, "Ask the person a question and wait for the answer.", &ask),
        meta_tool(
            TOOL_READ,
            "Have the reader answer a question about handles you cannot read. \"want\" is the shape of the answer, as {\"kind\": \"choice\", \"v\": [\"yes\", \"no\"]} (kinds: choice, integer, date, datetime, text, record, list); it is not a JSON Schema. A choice, integer, date or datetime answer is returned to you to read; an answer in words (text, or a record or list holding text) is returned as a handle you cannot read, so use it only as an argument.",
            &read,
        ),
        meta_tool(
            TOOL_FINISH,
            "The task is done. \"show\" lists text handles to put in front of the person as content, in this order; you cannot read them.",
            &finish,
        ),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The planner is deterministic enough to repeat: no sampling noise it does not need.
pub(crate) fn control() -> ChatControl {
    ChatControl::new()
        .with_tool_choice(ToolChoice::Auto)
        .with_tool_calls(ToolParallelism::Many)
        .with_max_output(Knob::Off)
        .with_reasoning(Reasoning::EngineDefault)
        .with_sampling(Knob::Off)
}
