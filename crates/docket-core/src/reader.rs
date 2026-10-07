//! The reader contract: the planner asks, the quarantined reader answers with a typed value
//! only. The reader runs in its own process (`readerd`), has no tools, and answers under a
//! schema. Its result is labelled with the join of its inputs; any text inside it reaches the
//! planner as a new handle.

use crate::ids::{ChoiceId, Handle, ParamName};
use crate::planner::HandleShape;
use crate::units::CharCount;
use crate::value::Value;
use model_provider::{ChoiceText, Field, FieldName, Shape};
use porter_core::Count;
use prov::{EntityId, Quarantined, SessionId};
use serde::{Deserialize, Serialize};
use std::future::Future;

/// What the planner asks the reader to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReaderAsk {
    /// The values to read, by handle.
    pub inputs: Vec<Handle>,
    /// The shape of the answer.
    pub want: ValueSchema,
    /// What to do with them.
    pub task: ReaderTask,
}

/// The planner's instruction is a closed task plus a schema, never free text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReaderTask {
    /// Pick among the choices.
    Classify,
    /// Pull out the fields.
    Extract,
    /// Summarise.
    Summarise,
    /// Compare.
    Compare,
}

/// The shape of an answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ValueSchema {
    /// One of these.
    Choice(Vec<ChoiceId>),
    /// A whole number in `min..=max`.
    Integer {
        /// The smallest.
        min: i64,
        /// The largest.
        max: i64,
    },
    /// A calendar date.
    Date,
    /// An instant.
    DateTime,
    /// Some of these things, and only these.
    EntitiesAmong(Vec<EntityId>),
    /// Words, up to `max` characters.
    Text {
        /// The longest.
        max: CharCount,
    },
    /// Named fields.
    Record(Vec<(ParamName, ValueSchema)>),
    /// A list.
    List {
        /// Each element.
        of: Box<ValueSchema>,
        /// At most this many.
        max: Count,
    },
}

/// Why a value does not fit a schema.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SchemaFault {
    /// The wrong kind of value.
    #[error("wrong type")]
    WrongType,
    /// A number outside its range.
    #[error("out of range")]
    OutOfRange,
    /// Not one of the allowed options or things.
    #[error("not in the allowed set")]
    NotInSet,
    /// Longer than allowed.
    #[error("too long")]
    TooLong,
    /// More elements than allowed.
    #[error("too many elements")]
    TooMany,
    /// A field is missing.
    #[error("missing field {0}")]
    MissingField(ParamName),
    /// A field the schema does not name.
    #[error("unknown field {0}")]
    UnknownField(ParamName),
    /// The schema has no form in the structured-output vocabulary.
    #[error("schema cannot be rendered as a shape")]
    NotRepresentable,
}

/// Whether `value` is an answer of shape `schema`.
pub fn conforms(value: &Value, schema: &ValueSchema) -> Result<(), SchemaFault> {
    match (value, schema) {
        (Value::Choice(id), ValueSchema::Choice(options)) => options
            .contains(id)
            .then_some(())
            .ok_or(SchemaFault::NotInSet),
        (Value::Integer(n), ValueSchema::Integer { min, max }) => (*min..=*max)
            .contains(n)
            .then_some(())
            .ok_or(SchemaFault::OutOfRange),
        (Value::Date(_), ValueSchema::Date) | (Value::DateTime(_), ValueSchema::DateTime) => Ok(()),
        (Value::Entities(found), ValueSchema::EntitiesAmong(allowed)) => found
            .iter()
            .all(|e| allowed.contains(e))
            .then_some(())
            .ok_or(SchemaFault::NotInSet),
        (Value::Entity(found), ValueSchema::EntitiesAmong(allowed)) => allowed
            .contains(found)
            .then_some(())
            .ok_or(SchemaFault::NotInSet),
        (Value::Text(t), ValueSchema::Text { max }) => (t.chars().count() <= max.0 as usize)
            .then_some(())
            .ok_or(SchemaFault::TooLong),
        (Value::Record(fields), ValueSchema::Record(wanted)) => {
            if let Some(extra) = fields.keys().find(|k| !wanted.iter().any(|(n, _)| &n == k)) {
                return Err(SchemaFault::UnknownField(extra.clone()));
            }
            wanted.iter().try_for_each(|(name, schema)| {
                let value = fields
                    .get(name)
                    .ok_or_else(|| SchemaFault::MissingField(name.clone()))?;
                conforms(value, schema)
            })
        }
        (Value::List(items), ValueSchema::List { of, max }) => {
            if items.len() > max.0 as usize {
                return Err(SchemaFault::TooMany);
            }
            items.iter().try_for_each(|item| conforms(item, of))
        }
        _ => Err(SchemaFault::WrongType),
    }
}

/// How an entity is named inside a choice shape: `app/kind/key`.
pub fn entity_choice_text(id: &EntityId) -> String {
    format!("{}/{}/{}", id.app, id.kind, id.key)
}

impl ValueSchema {
    /// The schema in stoker's structured-output vocabulary, so inferd renders it for the engine
    /// (JSON Schema, GBNF or a regex) and owns validation, repair and retry. Entities are named
    /// by [`entity_choice_text`].
    pub fn shape(&self) -> Result<Shape, SchemaFault> {
        let count = |n: usize| model_provider::Count(u32::try_from(n).unwrap_or(u32::MAX));
        match self {
            ValueSchema::Choice(ids) => Ok(Shape::Choice(
                ids.iter()
                    .map(|i| ChoiceText(i.as_str().to_owned()))
                    .collect(),
            )),
            ValueSchema::Integer { min, max } => Ok(Shape::Integer {
                min: *min,
                max: *max,
            }),
            ValueSchema::Date => Ok(Shape::Date),
            ValueSchema::DateTime => Ok(Shape::DateTime),
            ValueSchema::EntitiesAmong(ids) => Ok(Shape::List {
                of: Box::new(Shape::Choice(
                    ids.iter()
                        .map(|e| ChoiceText(entity_choice_text(e)))
                        .collect(),
                )),
                max: count(ids.len()),
            }),
            ValueSchema::Text { max } => Ok(Shape::Text {
                max: model_provider::CharCount(max.0),
            }),
            ValueSchema::Record(fields) => fields
                .iter()
                .map(|(name, schema)| {
                    let name =
                        FieldName::new(name.as_str()).map_err(|_| SchemaFault::NotRepresentable)?;
                    Ok(Field {
                        name,
                        shape: schema.shape()?,
                    })
                })
                .collect::<Result<Vec<_>, SchemaFault>>()
                .map(Shape::Record),
            ValueSchema::List { of, max } => Ok(Shape::List {
                of: Box::new(of.shape()?),
                max: count(max.0 as usize),
            }),
        }
    }
}

impl ValueSchema {
    /// The schema as JSON Schema text (stoker's `Shape::to_json_schema`, plain dialect): what the
    /// reader sends inferd as the shape of its reply.
    pub fn json_schema(&self) -> Result<String, SchemaFault> {
        Ok(self
            .shape()?
            .to_json_schema(model_provider::SchemaDialect::Plain)
            .0
            .as_str()
            .to_owned())
    }
}

/// Why the reader could not answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ReaderError {
    /// The answer did not fit the schema.
    #[error("answer outside the schema")]
    OutOfSchema(SchemaFault),
    /// The reply did not parse.
    #[error("unparseable reply")]
    Unparseable,
    /// The model refused.
    #[error("refused")]
    Refused,
    /// No model could be reached.
    #[error("model unavailable")]
    ModelUnavailable,
}

/// Why a read produced no answer the planner can use: told to it as a line of its history when
/// it can write the read again, a failure of the turn when no reader answers at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ReadFault {
    /// `inputs` was not a non-empty list of handle numbers.
    Inputs,
    /// `task` was not one of the four.
    Task,
    /// `want` was not a shape of the answer.
    Want,
    /// An input is a handle this session does not hold.
    NotHeld,
    /// An input is held but is a thing or a file, not text: the planner reads it first, through
    /// the app's own action (a call the policy sees), and gives the handle that returns.
    NotText {
        /// The input.
        handle: Handle,
        /// What it holds.
        shape: HandleShape,
    },
    /// The reader's answer did not fit `want`.
    OutOfSchema(SchemaFault),
    /// The reader's reply could not be read at all.
    Unparseable,
    /// The reader refused.
    Refused,
    /// No reader or model could answer: not the planner's mistake.
    Unavailable,
}

impl From<ReaderError> for ReadFault {
    fn from(error: ReaderError) -> Self {
        match error {
            ReaderError::OutOfSchema(fault) => ReadFault::OutOfSchema(fault),
            ReaderError::Unparseable => ReadFault::Unparseable,
            ReaderError::Refused => ReadFault::Refused,
            ReaderError::ModelUnavailable => ReadFault::Unavailable,
        }
    }
}

/// What `readerd` implements; `intentd` calls it over `Reader1`. The inputs are quarantined:
/// only the reader's host holds the key that opens them.
pub trait Reader: Send + Sync {
    /// Reads the inputs under the schema and answers with a value that fits it. `session` is the
    /// session the ask's handles are held in: the router passes it, a reader in another process
    /// resolves the handles itself through it, and one that is handed the text ignores it.
    fn extract(
        &self,
        session: &SessionId,
        ask: ReaderAsk,
        inputs: Vec<Quarantined<String>>,
    ) -> impl Future<Output = Result<Value, ReaderError>> + Send;
}
