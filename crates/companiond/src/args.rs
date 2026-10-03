//! The arguments of a tool call the model made, read by the types the action declares. The model
//! speaks JSON; the router wants `Args` and a target. Every value is labelled as the planner's
//! own (the router derives the real labels itself and trusts none of them), and anything that
//! does not fit its declared type is a fault before the router is asked.

use crate::catalogue::{CatalogueTool, TARGET};
use docket_core::{
    Args, ChoiceId, CivilDate, Decimal, FileRef, Handle, ParamDecl, ParamName, ParamNeed,
    ParamType, Seconds, TargetKind, TargetValue, Value,
};
use prov::{EntityId, Integrity, Label, Labelled, ModelRole, Source, UnixSeconds};
use serde_json::Value as Json;
use std::collections::BTreeSet;

/// Why a tool call's arguments are not the action's.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ArgsFault {
    /// The arguments are not one JSON object.
    #[error("the arguments are not an object")]
    NotAnObject,
    /// A name the action does not declare.
    #[error("unknown argument {0}")]
    Unknown(String),
    /// A required parameter is absent.
    #[error("missing argument {0}")]
    Missing(ParamName),
    /// A value is not of its declared type.
    #[error("argument {0} has the wrong type")]
    WrongType(String),
}

/// What a tool call asks of the router, short of the origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadCall {
    /// What it acts on.
    pub target: TargetValue,
    /// Its arguments.
    pub args: Args,
}

/// The label of what a model wrote: its own words, untrusted, from the planner.
pub fn planner_label() -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: prov::Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::Model(ModelRole::Planner)]),
    }
}

fn wrong(name: &str) -> ArgsFault {
    ArgsFault::WrongType(name.to_owned())
}

/// `{ "handle": n }`.
fn handle_of(json: &Json) -> Option<Handle> {
    let object = json.as_object()?;
    (object.len() == 1)
        .then(|| object.get("handle")?.as_u64())
        .flatten()
        .map(Handle)
}

fn entity_of(json: &Json) -> Option<EntityId> {
    serde_json::from_value(json.clone()).ok()
}

fn text_of(json: &Json) -> Option<String> {
    json.as_str().map(str::to_owned)
}

fn read_value(ty: &ParamType, json: &Json, name: &str) -> Result<Value, ArgsFault> {
    if let Some(handle) = handle_of(json)
        && !matches!(ty, ParamType::Decimal { .. } | ParamType::Date)
    {
        return Ok(Value::Handle(handle));
    }
    let bad = || wrong(name);
    match ty {
        ParamType::Text { .. } => text_of(json).map(Value::Text).ok_or_else(bad),
        ParamType::Url => text_of(json).map(Value::Url).ok_or_else(bad),
        ParamType::File => text_of(json)
            .and_then(|t| FileRef::parse(&t).ok())
            .map(Value::File)
            .ok_or_else(bad),
        ParamType::Integer { min, max } => json
            .as_i64()
            .filter(|n| (*min..=*max).contains(n))
            .map(Value::Integer)
            .ok_or_else(bad),
        ParamType::DateTime => json
            .as_i64()
            .map(|n| Value::DateTime(UnixSeconds(n)))
            .ok_or_else(bad),
        ParamType::Duration => json
            .as_u64()
            .and_then(|n| u32::try_from(n).ok())
            .map(|n| Value::Duration(Seconds(n)))
            .ok_or_else(bad),
        ParamType::Decimal { scale } => serde_json::from_value::<Decimal>(json.clone())
            .ok()
            .filter(|d| d.scale == *scale)
            .map(Value::Decimal)
            .ok_or_else(bad),
        ParamType::Date => serde_json::from_value::<CivilDate>(json.clone())
            .ok()
            .map(Value::Date)
            .ok_or_else(bad),
        ParamType::Choice(options) => text_of(json)
            .and_then(|t| ChoiceId::parse(&t).ok())
            .filter(|id| options.iter().any(|o| &o.id == id))
            .map(Value::Choice)
            .ok_or_else(bad),
        ParamType::Entity(kind) => entity_of(json)
            .filter(|e| &e.kind == kind)
            .map(Value::Entity)
            .ok_or_else(bad),
        ParamType::Entities(kind) => entities(json, Some(kind), name),
        ParamType::Dynamic(_) => match entity_of(json) {
            Some(entity) => Ok(Value::Entity(entity)),
            None => text_of(json)
                .and_then(|t| ChoiceId::parse(&t).ok())
                .map(Value::Choice)
                .ok_or_else(bad),
        },
    }
}

/// A list of things: plain entities when every one is named, a list of values when any is a
/// handle (the router resolves each with the label it was minted with).
fn entities(json: &Json, kind: Option<&prov::EntityKind>, name: &str) -> Result<Value, ArgsFault> {
    let items = json.as_array().ok_or_else(|| wrong(name))?;
    let mut named = Vec::new();
    let mut mixed = Vec::new();
    for item in items {
        if let Some(handle) = handle_of(item) {
            mixed.push(Value::Handle(handle));
            continue;
        }
        let entity = entity_of(item)
            .filter(|e| kind.is_none_or(|k| &e.kind == k))
            .ok_or_else(|| wrong(name))?;
        named.push(entity.clone());
        mixed.push(Value::Entity(entity));
    }
    if named.len() == items.len() {
        Ok(Value::Entities(named))
    } else {
        Ok(Value::List(mixed))
    }
}

fn read_target(on: &TargetKind, json: Option<&Json>) -> Result<TargetValue, ArgsFault> {
    let Some(json) = json else {
        return Ok(TargetValue::Nothing);
    };
    match on {
        TargetKind::Nothing | TargetKind::Text | TargetKind::Files => Err(wrong(TARGET)),
        TargetKind::One(kind) => entity_of(json)
            .filter(|e| &e.kind == kind)
            .map(|e| TargetValue::Entities(vec![e]))
            .ok_or_else(|| wrong(TARGET)),
        TargetKind::Many(kind) => match entities(json, Some(kind), TARGET)? {
            Value::Entities(es) => Ok(TargetValue::Entities(es)),
            _ => Err(wrong(TARGET)),
        },
    }
}

fn labelled(value: Value) -> Labelled<Value> {
    Labelled {
        value,
        label: planner_label(),
    }
}

/// Reads one tool call's JSON arguments.
pub fn read_call(tool: &CatalogueTool, json: &Json) -> Result<ReadCall, ArgsFault> {
    let object = json.as_object().ok_or(ArgsFault::NotAnObject)?;
    let declared = |n: &str| tool.decl.params.iter().find(|p| p.name.as_str() == n);
    if let Some(unknown) = object
        .keys()
        .find(|k| k.as_str() != TARGET && declared(k).is_none())
    {
        return Err(ArgsFault::Unknown(unknown.clone()));
    }
    let mut args = Args::new();
    for ParamDecl { name, ty, need, .. } in &tool.decl.params {
        match (object.get(name.as_str()), need) {
            (Some(json), _) => {
                args.insert(name.clone(), labelled(read_value(ty, json, name.as_str())?));
            }
            (None, ParamNeed::Required) => return Err(ArgsFault::Missing(name.clone())),
            (None, ParamNeed::Optional | ParamNeed::Defaulted(_)) => {}
        }
    }
    let target = read_target(&tool.decl.on, object.get(TARGET))?;
    Ok(ReadCall { target, args })
}
