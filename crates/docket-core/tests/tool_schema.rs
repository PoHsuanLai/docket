//! The JSON Schema of an action's arguments, pinned for three fixture actions.

mod support;

use docket_core::*;
use prov::Effect;
use serde_json::json;
use support::*;

fn param_of(name: &str, ty: ParamType, need: ParamNeed, sink: ArgSink) -> ParamDecl {
    ParamDecl {
        name: param(name),
        label: words("Field"),
        ty,
        need,
        sink,
    }
}

fn send() -> ActionDecl {
    let mut a = decl("mail.message.send", Effect::Outbound, UndoSupport::Token);
    a.params = vec![
        param_of(
            "to",
            ParamType::Entity(kind("mail.contact")),
            ParamNeed::Required,
            ArgSink::Recipient,
        ),
        param_of(
            "body",
            ParamType::Text {
                max: CharCount(500),
                lines: Lines::Many,
            },
            ParamNeed::Required,
            ArgSink::Body,
        ),
        param_of(
            "subject",
            ParamType::Text {
                max: CharCount(80),
                lines: Lines::One,
            },
            ParamNeed::Optional,
            ArgSink::Inert,
        ),
    ];
    a
}

fn move_files() -> ActionDecl {
    let mut a = decl("files.file.move", Effect::UndoableWrite, UndoSupport::Token);
    a.params = vec![
        param_of("into", ParamType::File, ParamNeed::Required, ArgSink::Path),
        param_of(
            "label",
            ParamType::Choice(vec![
                ChoiceDecl {
                    id: ChoiceId::parse("keep").expect("id"),
                    label: words("Keep"),
                },
                ChoiceDecl {
                    id: ChoiceId::parse("drop").expect("id"),
                    label: words("Drop"),
                },
            ]),
            ParamNeed::Defaulted(Value::Choice(ChoiceId::parse("keep").expect("id"))),
            ArgSink::Inert,
        ),
        param_of(
            "also",
            ParamType::Entities(kind("files.file")),
            ParamNeed::Optional,
            ArgSink::Inert,
        ),
    ];
    a
}

fn remind() -> ActionDecl {
    let mut a = decl(
        "cal.event.remind",
        Effect::UndoableWrite,
        UndoSupport::Token,
    );
    a.params = vec![
        param_of(
            "count",
            ParamType::Integer { min: 1, max: 9 },
            ParamNeed::Required,
            ArgSink::Inert,
        ),
        param_of("on", ParamType::Date, ParamNeed::Optional, ArgSink::Inert),
        param_of(
            "at",
            ParamType::DateTime,
            ParamNeed::Optional,
            ArgSink::Inert,
        ),
    ];
    a
}

fn entity_schema(kind: &str) -> serde_json::Value {
    json!({
        "type": "object",
        "properties": {
            "app": { "type": "string" },
            "kind": { "const": kind },
            "key": { "type": "string" }
        },
        "required": ["app", "kind", "key"],
        "additionalProperties": false
    })
}

fn handle_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "properties": { "handle": { "type": "integer", "minimum": 0 } },
        "required": ["handle"],
        "additionalProperties": false
    })
}

#[test]
fn tool_schema_snapshot() {
    let cases = [
        (
            "an outbound send: an entity, many-line text, a one-line optional",
            send(),
            json!({
                "type": "object",
                "properties": {
                    "to": {
                        "anyOf": [entity_schema("mail.contact"), handle_schema()],
                        "description": "Field"
                    },
                    "body": {
                        "anyOf": [
                            { "type": "string", "maxLength": 500 },
                            handle_schema()
                        ],
                        "description": "Field"
                    },
                    "subject": {
                        "anyOf": [
                            { "type": "string", "maxLength": 80, "pattern": "^[^\\n]*$" },
                            handle_schema()
                        ],
                        "description": "Field"
                    }
                },
                "required": ["to", "body"],
                "additionalProperties": false
            }),
        ),
        (
            "a file move: a file, a defaulted choice, several entities",
            move_files(),
            json!({
                "type": "object",
                "properties": {
                    "into": {
                        "anyOf": [{ "type": "string" }, handle_schema()],
                        "description": "Field"
                    },
                    "label": {
                        "type": "string",
                        "enum": ["keep", "drop"],
                        "description": "Field"
                    },
                    "also": {
                        "type": "array",
                        "items": { "anyOf": [entity_schema("files.file"), handle_schema()] },
                        "description": "Field"
                    }
                },
                "required": ["into"],
                "additionalProperties": false
            }),
        ),
        (
            "a reminder: a bounded integer, a date and an instant",
            remind(),
            json!({
                "type": "object",
                "properties": {
                    "count": {
                        "type": "integer",
                        "minimum": 1,
                        "maximum": 9,
                        "description": "Field"
                    },
                    "on": {
                        "type": "object",
                        "properties": {
                            "year": { "type": "integer" },
                            "month": { "type": "integer", "minimum": 1, "maximum": 12 },
                            "day": { "type": "integer", "minimum": 1, "maximum": 31 }
                        },
                        "required": ["year", "month", "day"],
                        "additionalProperties": false,
                        "description": "Field"
                    },
                    "at": {
                        "type": "integer",
                        "description": "Field"
                    }
                },
                "required": ["count"],
                "additionalProperties": false
            }),
        ),
    ];
    for (name, action, want) in cases {
        assert_eq!(tool_schema(&action).0, want, "case: {name}");
    }
}

#[test]
fn an_action_with_no_parameters_takes_an_empty_object() {
    let a = decl(
        "mail.thread.archive",
        Effect::UndoableWrite,
        UndoSupport::Token,
    );
    assert_eq!(
        tool_schema(&a).0,
        json!({
            "type": "object",
            "properties": {},
            "required": [],
            "additionalProperties": false
        })
    );
}
