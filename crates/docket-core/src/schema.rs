//! The JSON Schema of an action's arguments: the one source for the planner's grammar and for
//! MCP. The planner supplies entity arguments as the `{app, kind, key}` object of an `EntityId`
//! and handles as `{ "handle": n }`; the router labels everything it receives.

use crate::manifest::{ActionDecl, ParamDecl, ParamNeed};
use crate::value::{Lines, ParamType};
use prov::EntityKind;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// A JSON Schema document for one action's arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolSchema(pub serde_json::Value);

/// The schema of `action`'s arguments: an object with one property per parameter, the required
/// ones listed, no additional properties. A parameter a planner may fill from something it
/// holds only by handle also accepts `{ "handle": n }`; entities are the `{app, kind, key}`
/// object of an `EntityId`.
pub fn tool_schema(action: &ActionDecl) -> ToolSchema {
    let properties: Map<String, Value> = action
        .params
        .iter()
        .map(|p| (p.name.as_str().to_owned(), property(p)))
        .collect();
    let required: Vec<&str> = action
        .params
        .iter()
        .filter(|p| matches!(p.need, ParamNeed::Required))
        .map(|p| p.name.as_str())
        .collect();
    ToolSchema(json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false,
    }))
}

fn property(param: &ParamDecl) -> Value {
    let mut schema = of_type(&param.ty);
    if let Value::Object(fields) = &mut schema {
        fields.insert("description".into(), json!(param.label.as_str()));
    }
    schema
}

fn handle() -> Value {
    json!({
        "type": "object",
        "properties": { "handle": { "type": "integer", "minimum": 0 } },
        "required": ["handle"],
        "additionalProperties": false,
    })
}

fn or_handle(inner: Value) -> Value {
    json!({ "anyOf": [inner, handle()] })
}

fn entity(kind: &EntityKind) -> Value {
    json!({
        "type": "object",
        "properties": {
            "app": { "type": "string" },
            "kind": { "const": kind.as_str() },
            "key": { "type": "string" },
        },
        "required": ["app", "kind", "key"],
        "additionalProperties": false,
    })
}

fn of_type(ty: &ParamType) -> Value {
    match ty {
        ParamType::Text { max, lines } => {
            let mut text = json!({ "type": "string", "maxLength": max.0 });
            if *lines == Lines::One
                && let Value::Object(fields) = &mut text
            {
                fields.insert("pattern".into(), json!("^[^\\n]*$"));
            }
            or_handle(text)
        }
        ParamType::Integer { min, max } => {
            json!({ "type": "integer", "minimum": min, "maximum": max })
        }
        ParamType::Decimal { scale } => json!({
            "type": "object",
            "properties": {
                "units": { "type": "integer" },
                "scale": { "const": scale.0 },
            },
            "required": ["units", "scale"],
            "additionalProperties": false,
        }),
        ParamType::Date => json!({
            "type": "object",
            "properties": {
                "year": { "type": "integer" },
                "month": { "type": "integer", "minimum": 1, "maximum": 12 },
                "day": { "type": "integer", "minimum": 1, "maximum": 31 },
            },
            "required": ["year", "month", "day"],
            "additionalProperties": false,
        }),
        ParamType::DateTime => json!({ "type": "integer" }),
        ParamType::Duration => json!({ "type": "integer", "minimum": 0 }),
        ParamType::Choice(options) => {
            let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
            json!({ "type": "string", "enum": ids })
        }
        ParamType::Entity(kind) => or_handle(entity(kind)),
        ParamType::Entities(kind) => json!({
            "type": "array",
            "items": or_handle(entity(kind)),
        }),
        ParamType::File => or_handle(json!({ "type": "string" })),
        ParamType::Url => or_handle(json!({ "type": "string", "format": "uri" })),
        ParamType::Dynamic(_) => json!({ "type": "string" }),
    }
}
