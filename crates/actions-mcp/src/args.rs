//! JSON arguments to typed ones, by each parameter's declared type. Pure: nothing here talks to
//! the router. Every value comes out labelled with the label it is given (the edge passes
//! `mcp_label(client)`), and an argument the manifest does not declare is refused rather than
//! ignored. The target of an action is the one extra key, `target`, and its shape follows the
//! action's `on`.
//!
//! The shapes are `tool_schema`'s: an entity is `{app, kind, key}`, a decimal `{units, scale}`,
//! a date `{year, month, day}`, an instant or a duration whole seconds, and a value the client
//! holds only by handle is `{ "handle": n }`.

use crate::fault::{ArgsFault, TargetFault, Why};
use docket_core::{
    ActionDecl, Args, CharCount, CivilDate, Decimal, FileRef, Handle, Lines, ParamDecl, ParamNeed,
    ParamType, Scale, Seconds, TargetKind, TargetValue, Value,
};
use porter_core::AppName;
use prov::{EntityId, EntityKey, EntityKind, Label, Labelled, UnixSeconds};
use serde_json::{Map, Value as Json};

/// The key a client names an action's target under.
pub const TARGET_KEY: &str = "target";

type Read<T> = Result<T, Why>;

fn handle(json: &Json) -> Option<Handle> {
    let object = json.as_object()?;
    match (object.len(), object.get("handle")) {
        (1, Some(n)) => n.as_u64().map(Handle),
        _ => None,
    }
}

fn entity(kind: &EntityKind, json: &Json) -> Read<EntityId> {
    let object = json.as_object().filter(|o| o.len() == 3).ok_or(Why::Type)?;
    let text = |name: &str| object.get(name).and_then(Json::as_str).ok_or(Why::Type);
    let app = AppName::parse(text("app")?).map_err(|_| Why::Range)?;
    let named = EntityKind::parse(text("kind")?).map_err(|_| Why::Range)?;
    let key = EntityKey::parse(text("key")?).map_err(|_| Why::Range)?;
    if &named == kind {
        Ok(EntityId {
            app,
            kind: named,
            key,
        })
    } else {
        Err(Why::Range)
    }
}

fn text(max: CharCount, lines: Lines, json: &Json) -> Read<String> {
    let body = json.as_str().ok_or(Why::Type)?;
    let too_long = body.chars().count() > max.0 as usize;
    let multi = lines == Lines::One && body.contains('\n');
    if too_long || multi {
        Err(Why::Range)
    } else {
        Ok(body.to_owned())
    }
}

fn integer(json: &Json) -> Read<i64> {
    json.as_i64().ok_or(Why::Type)
}

fn small<T: TryFrom<i64>>(n: i64) -> Read<T> {
    T::try_from(n).map_err(|_| Why::Range)
}

fn field(object: &Map<String, Json>, name: &str) -> Read<i64> {
    object.get(name).ok_or(Why::Type).and_then(integer)
}

fn decimal(scale: Scale, json: &Json) -> Read<Decimal> {
    let object = json.as_object().filter(|o| o.len() == 2).ok_or(Why::Type)?;
    let units = field(object, "units")?;
    let given = Scale(small(field(object, "scale")?)?);
    if given == scale {
        Ok(Decimal { units, scale })
    } else {
        Err(Why::Range)
    }
}

fn date(json: &Json) -> Read<CivilDate> {
    let object = json.as_object().filter(|o| o.len() == 3).ok_or(Why::Type)?;
    let day = CivilDate {
        year: small(field(object, "year")?)?,
        month: small(field(object, "month")?)?,
        day: small(field(object, "day")?)?,
    };
    if (1..=12).contains(&day.month) && (1..=31).contains(&day.day) {
        Ok(day)
    } else {
        Err(Why::Range)
    }
}

fn url(raw: &str) -> Read<String> {
    match raw.split_once("://") {
        Some((scheme, rest)) if !scheme.is_empty() && !rest.is_empty() => Ok(raw.to_owned()),
        _ => Err(Why::Range),
    }
}

/// One JSON value as a `ty`.
fn value(ty: &ParamType, json: &Json) -> Read<Value> {
    if let Some(held) = handle(json) {
        return match ty {
            ParamType::Text { .. }
            | ParamType::Entity(_)
            | ParamType::File
            | ParamType::Url
            | ParamType::Dynamic(_) => Ok(Value::Handle(held)),
            _ => Err(Why::Type),
        };
    }
    match ty {
        ParamType::Text { max, lines } => text(*max, *lines, json).map(Value::Text),
        ParamType::Dynamic(_) => text(CharCount(512), Lines::One, json).map(Value::Text),
        ParamType::Integer { min, max } => integer(json)
            .and_then(|n| (*min..=*max).contains(&n).then_some(n).ok_or(Why::Range))
            .map(Value::Integer),
        ParamType::Decimal { scale } => decimal(*scale, json).map(Value::Decimal),
        ParamType::Date => date(json).map(Value::Date),
        ParamType::DateTime => integer(json).map(|n| Value::DateTime(UnixSeconds(n))),
        ParamType::Duration => integer(json)
            .and_then(small)
            .map(|n| Value::Duration(Seconds(n))),
        ParamType::Choice(options) => {
            let id = json.as_str().ok_or(Why::Type)?;
            options
                .iter()
                .find(|o| o.id.as_str() == id)
                .map(|o| Value::Choice(o.id.clone()))
                .ok_or(Why::Range)
        }
        ParamType::Entity(kind) => entity(kind, json).map(Value::Entity),
        ParamType::Entities(kind) => json
            .as_array()
            .ok_or(Why::Type)?
            .iter()
            .map(|item| entity(kind, item))
            .collect::<Read<Vec<_>>>()
            .map(Value::Entities),
        ParamType::File => json
            .as_str()
            .ok_or(Why::Type)
            .and_then(|p| FileRef::parse(p).map_err(|_| Why::Range))
            .map(Value::File),
        ParamType::Url => json.as_str().ok_or(Why::Type).and_then(url).map(Value::Url),
    }
}

fn argument(param: &ParamDecl, json: &Json) -> Result<Value, ArgsFault> {
    value(&param.ty, json).map_err(|why| ArgsFault::Wrong {
        param: param.name.clone(),
        why,
    })
}

fn files(json: &Json) -> Read<Vec<FileRef>> {
    json.as_array()
        .ok_or(Why::Type)?
        .iter()
        .map(|item| {
            item.as_str()
                .ok_or(Why::Type)
                .and_then(|p| FileRef::parse(p).map_err(|_| Why::Range))
        })
        .collect()
}

/// What the action acts on, from the `target` key (absent: none).
pub fn target(decl: &ActionDecl, given: Option<&Json>) -> Result<TargetValue, TargetFault> {
    let bad = |_: Why| TargetFault::Malformed;
    match (&decl.on, given) {
        (TargetKind::Nothing, None) => Ok(TargetValue::Nothing),
        (TargetKind::Nothing, Some(_)) => Err(TargetFault::Unexpected),
        (TargetKind::Text, _) => Err(TargetFault::NotNameable),
        (_, None) => Err(TargetFault::Missing),
        (TargetKind::One(kind), Some(json)) => entity(kind, json)
            .map(|e| TargetValue::Entities(vec![e]))
            .map_err(bad),
        (TargetKind::Many(kind), Some(json)) => json
            .as_array()
            .filter(|items| !items.is_empty())
            .ok_or(TargetFault::Malformed)?
            .iter()
            .map(|item| entity(kind, item))
            .collect::<Read<Vec<_>>>()
            .map(TargetValue::Entities)
            .map_err(bad),
        (TargetKind::Files, Some(json)) => files(json)
            .ok()
            .filter(|f| !f.is_empty())
            .map(TargetValue::Files)
            .ok_or(TargetFault::Malformed),
    }
}

/// The target and the arguments of one call, every argument labelled `label`.
pub fn read_call(
    decl: &ActionDecl,
    arguments: Option<&Map<String, Json>>,
    label: &Label,
) -> Result<(TargetValue, Args), ArgsFault> {
    let empty = Map::new();
    let given = arguments.unwrap_or(&empty);
    for name in given.keys() {
        let declared = decl.params.iter().any(|p| p.name.as_str() == name);
        if !declared && name != TARGET_KEY {
            return Err(ArgsFault::Unknown(name.clone()));
        }
    }
    let target = target(decl, given.get(TARGET_KEY))?;
    let mut args = Args::new();
    for param in &decl.params {
        match given.get(param.name.as_str()) {
            Some(json) => {
                args.insert(
                    param.name.clone(),
                    Labelled {
                        value: argument(param, json)?,
                        label: label.clone(),
                    },
                );
            }
            None if param.need == ParamNeed::Required => {
                return Err(ArgsFault::Missing(param.name.clone()));
            }
            None => {}
        }
    }
    Ok((target, args))
}
