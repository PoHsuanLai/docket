//! `describe`: the JSON Schema of an action's parameters as the command line takes them, built
//! from stoker's `Shape` and rendered by `Shape::to_json_schema`. The command line's forms are
//! the schema's forms: a thing is the text `<kind>:<key>`, a link or a path is text, a decimal
//! and a duration are the text typed.

use crate::exit::{Exit, Failure};
use crate::resolve::{short_name, slug};
use docket_core::{ActionDecl, Manifest, ParamDecl, ParamNeed, ParamType};
use model_provider::{CharCount, ChoiceText, Count, Field, FieldName, SchemaDialect, Shape};
use serde_json::{Value, json};

/// The longest list of things a flag may repeat to.
const MOST_THINGS: u32 = 1000;

fn text(max: u32) -> Shape {
    Shape::Text {
        max: CharCount(max),
    }
}

fn shape_of_type(ty: &ParamType) -> Shape {
    match ty {
        ParamType::Text { max, .. } => text(max.0),
        ParamType::Integer { min, max } => Shape::Integer {
            min: *min,
            max: *max,
        },
        ParamType::Date => Shape::Date,
        ParamType::DateTime => Shape::DateTime,
        ParamType::Choice(options) => Shape::Choice(
            options
                .iter()
                .map(|o| ChoiceText(o.id.as_str().to_owned()))
                .collect(),
        ),
        ParamType::Entities(_) => Shape::List {
            of: Box::new(text(600)),
            max: Count(MOST_THINGS),
        },
        ParamType::Entity(_) => text(600),
        ParamType::Decimal { .. } | ParamType::Duration => text(40),
        ParamType::File => text(4096),
        ParamType::Url => text(2048),
        ParamType::Dynamic(_) => text(512),
    }
}

fn field(param: &ParamDecl) -> Result<Field, Failure> {
    let name = FieldName::new(param.name.as_str()).map_err(|_| {
        Failure::new(
            Exit::Usage,
            format!("{} cannot be described as a schema field", param.name),
        )
    })?;
    let shape = shape_of_type(&param.ty);
    Ok(Field {
        name,
        shape: match param.need {
            ParamNeed::Required => shape,
            ParamNeed::Optional | ParamNeed::Defaulted(_) => Shape::Optional(Box::new(shape)),
        },
    })
}

/// The shape of an action's arguments: one field per parameter, optional ones optional.
pub fn shape(action: &ActionDecl) -> Result<Shape, Failure> {
    action
        .params
        .iter()
        .map(field)
        .collect::<Result<Vec<_>, _>>()
        .map(Shape::Record)
}

/// The JSON Schema of an action's arguments.
pub fn json_schema(action: &ActionDecl) -> Result<Value, Failure> {
    let rendered = shape(action)?.to_json_schema(SchemaDialect::Plain);
    serde_json::from_str(rendered.0.as_str())
        .map_err(|_| Failure::new(Exit::Usage, "the schema did not render"))
}

fn type_word(ty: &ParamType) -> &'static str {
    match ty {
        ParamType::Text { .. } => "text",
        ParamType::Integer { .. } => "integer",
        ParamType::Decimal { .. } => "decimal",
        ParamType::Date => "date",
        ParamType::DateTime => "datetime",
        ParamType::Duration => "duration",
        ParamType::Choice(_) => "choice",
        ParamType::Entity(_) => "entity",
        ParamType::Entities(_) => "entities",
        ParamType::File => "file",
        ParamType::Url => "url",
        ParamType::Dynamic(_) => "text",
    }
}

fn need_word(need: &ParamNeed) -> &'static str {
    match need {
        ParamNeed::Required => "required",
        ParamNeed::Optional => "optional",
        ParamNeed::Defaulted(_) => "defaulted",
    }
}

/// One parameter as a row: its flag, type, need and label.
pub fn param_row(param: &ParamDecl) -> Value {
    json!({
        "flag": format!("--{}", param.name.as_str().replace('_', "-")),
        "name": param.name,
        "type": type_word(&param.ty),
        "need": need_word(&param.need),
        "label": param.label,
    })
}

/// One action as a row, for `--list` and `describe`.
pub fn action_row(app: &Manifest, action: &ActionDecl) -> Value {
    json!({
        "action": short_name(app, action),
        "name": action.name,
        "label": action.label,
        "effect": action.effect,
        "on": action.on,
        "undo": action.undo,
        "dry_run": action.dry_run,
        "reach": action.reach,
        "params": action.params.iter().map(param_row).collect::<Vec<_>>(),
    })
}

/// What `describe` prints as JSON.
pub fn described(app: &Manifest, action: &ActionDecl) -> Result<Value, Failure> {
    let mut row = action_row(app, action);
    if let Value::Object(map) = &mut row {
        map.insert("vocab".into(), json!(docket_core::IntentsVocab::CURRENT));
        map.insert("app".into(), json!(slug(&app.app)));
        map.insert("schema".into(), json_schema(action)?);
    }
    Ok(row)
}
