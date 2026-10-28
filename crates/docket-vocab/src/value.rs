//! Values an action takes and returns, and the types a manifest declares for its parameters.
//! No floats: decimals and instants are integers with a unit.

use crate::ids::{ChoiceId, FileRef, Handle, ParamName, TextTargetRef};
use crate::units::{CharCount, Scale, Seconds};
use prov::{ActionName, EntityId, EntityKind, Labelled, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// A fixed-point number: `units` thousandths when `scale` is 3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Decimal {
    /// The number times ten to the `scale`.
    pub units: i64,
    /// How many digits sit after the point.
    pub scale: Scale,
}

/// A calendar date with no time zone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct CivilDate {
    /// The year.
    pub year: i16,
    /// 1 to 12.
    pub month: u8,
    /// 1 to 31.
    pub day: u8,
}

/// A value an action takes or returns. Text may be somebody else's words; its label says whose.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Value {
    /// Words.
    Text(String),
    /// A whole number.
    Integer(i64),
    /// A fixed-point number.
    Decimal(Decimal),
    /// A calendar date.
    Date(CivilDate),
    /// An instant.
    DateTime(UnixSeconds),
    /// A length of time.
    Duration(Seconds),
    /// One option of a choice.
    Choice(ChoiceId),
    /// One thing an app owns.
    Entity(EntityId),
    /// Several things of one kind.
    Entities(Vec<EntityId>),
    /// A file.
    File(FileRef),
    /// A link.
    Url(String),
    /// A value the router holds for the reader; resolved by the router with its label kept.
    Handle(Handle),
    /// Values in order.
    List(Vec<Value>),
    /// Named values.
    Record(BTreeMap<ParamName, Value>),
}

// A value may hold the person's mail or a recipient: Debug shows its shape and never its text.
impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Text(t) => write!(f, "Text(<{} bytes>)", t.len()),
            Value::Url(t) => write!(f, "Url(<{} bytes>)", t.len()),
            Value::Integer(n) => write!(f, "Integer({n})"),
            Value::Decimal(d) => write!(f, "Decimal({d:?})"),
            Value::Date(d) => write!(f, "Date({d:?})"),
            Value::DateTime(t) => write!(f, "DateTime({t:?})"),
            Value::Duration(s) => write!(f, "Duration({s:?})"),
            Value::Choice(c) => write!(f, "Choice({c})"),
            Value::Entity(e) => write!(f, "Entity({}/{})", e.app, e.kind),
            Value::Entities(es) => write!(f, "Entities(<{}>)", es.len()),
            Value::File(_) => f.write_str("File(<path>)"),
            Value::Handle(h) => write!(f, "Handle({})", h.0),
            Value::List(vs) => write!(f, "List(<{}>)", vs.len()),
            Value::Record(r) => write!(f, "Record(<{}>)", r.len()),
        }
    }
}

/// The arguments of one call, each with the provenance of where it came from.
pub type Args = BTreeMap<ParamName, Labelled<Value>>;

/// What a call acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TargetValue {
    /// Nothing in particular.
    Nothing,
    /// These things.
    Entities(Vec<EntityId>),
    /// Things the caller holds only by handle (a planner names what it was given as `#n`). The
    /// router resolves them to `Entities` before anything else reads the target.
    Handles(Vec<Handle>),
    /// A live text field.
    Text(TextTargetRef),
    /// These files.
    Files(Vec<FileRef>),
}

/// Whether a text parameter is one line or many.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lines {
    /// One line.
    One,
    /// Several.
    Many,
}

/// One option a choice parameter offers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChoiceDecl {
    /// Its id.
    pub id: ChoiceId,
    /// What the person reads.
    pub label: crate::ids::LabelText,
}

/// The type of one parameter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ParamType {
    /// Words, up to `max` characters.
    Text {
        /// The longest allowed.
        max: CharCount,
        /// One line or many.
        lines: Lines,
    },
    /// A whole number in `min..=max`.
    Integer {
        /// The smallest.
        min: i64,
        /// The largest.
        max: i64,
    },
    /// A fixed-point number.
    Decimal {
        /// Digits after the point.
        scale: Scale,
    },
    /// A calendar date.
    Date,
    /// An instant.
    DateTime,
    /// A length of time.
    Duration,
    /// One of these.
    Choice(Vec<ChoiceDecl>),
    /// One thing of a kind.
    Entity(EntityKind),
    /// Several things of a kind.
    Entities(EntityKind),
    /// A file.
    File,
    /// A link.
    Url,
    /// Options come from a Read action (`Suggest`).
    Dynamic(ActionName),
}
