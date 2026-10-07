//! The reader's reply, read back as a `Value` under the ask's schema. Nothing is believed as
//! written: each field is read as the type its schema names, an entity is only one the ask
//! offered, and the whole is checked with `conforms` before it is passed on.

use docket_core::when::{date, instant};
use docket_core::{
    ChoiceId, ParamName, ReaderError, SchemaFault, Value, ValueSchema, conforms, entity_choice_text,
};
use serde_json::Value as Json;
use std::collections::BTreeMap;

fn out_of(fault: SchemaFault) -> ReaderError {
    ReaderError::OutOfSchema(fault)
}

fn text_of(json: &Json) -> Result<&str, ReaderError> {
    json.as_str().ok_or(out_of(SchemaFault::WrongType))
}

/// One JSON value as the value its schema asks for.
fn read(json: &Json, schema: &ValueSchema) -> Result<Value, ReaderError> {
    match schema {
        ValueSchema::Choice(_) => ChoiceId::parse(text_of(json)?)
            .map(Value::Choice)
            .map_err(|_| out_of(SchemaFault::NotInSet)),
        ValueSchema::Integer { .. } => json
            .as_i64()
            .map(Value::Integer)
            .ok_or(out_of(SchemaFault::WrongType)),
        ValueSchema::Date => date(text_of(json)?)
            .map(Value::Date)
            .map_err(|_| out_of(SchemaFault::WrongType)),
        ValueSchema::DateTime => instant(text_of(json)?)
            .map(Value::DateTime)
            .map_err(|_| out_of(SchemaFault::WrongType)),
        ValueSchema::EntitiesAmong(offered) => {
            let named = json.as_array().ok_or(out_of(SchemaFault::WrongType))?;
            named
                .iter()
                .map(|item| {
                    let text = text_of(item)?;
                    offered
                        .iter()
                        .find(|e| entity_choice_text(e) == text)
                        .cloned()
                        .ok_or(out_of(SchemaFault::NotInSet))
                })
                .collect::<Result<Vec<_>, _>>()
                .map(Value::Entities)
        }
        ValueSchema::Text { .. } => text_of(json).map(|t| Value::Text(t.to_owned())),
        ValueSchema::Record(wanted) => {
            let object = json.as_object().ok_or(out_of(SchemaFault::WrongType))?;
            let mut fields = BTreeMap::new();
            for (name, field) in wanted {
                let found = object
                    .get(name.as_str())
                    .ok_or_else(|| out_of(SchemaFault::MissingField(name.clone())))?;
                fields.insert(name.clone(), read(found, field)?);
            }
            if let Some(extra) = object
                .keys()
                .find(|k| !wanted.iter().any(|(n, _)| n.as_str() == *k))
            {
                let name = ParamName::parse(extra).map_err(|_| out_of(SchemaFault::WrongType))?;
                return Err(out_of(SchemaFault::UnknownField(name)));
            }
            Ok(Value::Record(fields))
        }
        ValueSchema::List { of, .. } => json
            .as_array()
            .ok_or(out_of(SchemaFault::WrongType))?
            .iter()
            .map(|item| read(item, of))
            .collect::<Result<Vec<_>, _>>()
            .map(Value::List),
    }
}

/// The reply text as an answer to `want`: a choice is the bare option (quotes tolerated), a
/// record is the object, and anything else is the `answer` of an object. The result fits the
/// schema or is an error.
pub fn answer_of(reply: &str, want: &ValueSchema) -> Result<Value, ReaderError> {
    let value = match want {
        ValueSchema::Choice(_) => {
            let bare = reply.trim().trim_matches('"');
            read(&Json::String(bare.to_owned()), want)?
        }
        ValueSchema::Record(_) => {
            let json: Json = serde_json::from_str(reply).map_err(|_| ReaderError::Unparseable)?;
            read(&json, want)?
        }
        _ => {
            let json: Json = serde_json::from_str(reply).map_err(|_| ReaderError::Unparseable)?;
            let answer = json.get("answer").ok_or(ReaderError::Unparseable)?;
            read(answer, want)?
        }
    };
    conforms(&value, want).map_err(out_of)?;
    Ok(value)
}
