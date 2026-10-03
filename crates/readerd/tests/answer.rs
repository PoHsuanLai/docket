//! The reply read back under the ask's schema: each field as the type the schema names, an
//! entity only if the ask offered it, and nothing out of schema passed on.

use docket_core::*;
use prov::{EntityId, EntityKey, EntityKind};
use readerd::answer_of;
use std::collections::BTreeMap;

fn p(name: &str) -> ParamName {
    ParamName::parse(name).expect("param")
}

fn c(id: &str) -> ChoiceId {
    ChoiceId::parse(id).expect("choice")
}

fn thread(key: &str) -> EntityId {
    EntityId {
        app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
        kind: EntityKind::parse("mail.thread").expect("kind"),
        key: EntityKey::parse(key).expect("key"),
    }
}

fn record() -> ValueSchema {
    ValueSchema::Record(vec![
        (p("total"), ValueSchema::Integer { min: 0, max: 1000 }),
        (p("when"), ValueSchema::Date),
        (
            p("kind"),
            ValueSchema::Choice(vec![c("hotel"), c("flight")]),
        ),
    ])
}

#[test]
fn answers_table() {
    let among = ValueSchema::EntitiesAmong(vec![thread("t1"), thread("t2")]);
    let rows: Vec<(&str, &str, ValueSchema, Result<Value, ReaderError>)> = vec![
        (
            "a bare choice",
            "receipt",
            ValueSchema::Choice(vec![c("receipt"), c("other")]),
            Ok(Value::Choice(c("receipt"))),
        ),
        (
            "a quoted choice",
            "\"receipt\"\n",
            ValueSchema::Choice(vec![c("receipt"), c("other")]),
            Ok(Value::Choice(c("receipt"))),
        ),
        (
            "a choice nobody offered",
            "everything",
            ValueSchema::Choice(vec![c("receipt")]),
            Err(ReaderError::OutOfSchema(SchemaFault::NotInSet)),
        ),
        (
            "a wrapped integer",
            r#"{"answer": 4}"#,
            ValueSchema::Integer { min: 1, max: 5 },
            Ok(Value::Integer(4)),
        ),
        (
            "an integer out of range",
            r#"{"answer": 40}"#,
            ValueSchema::Integer { min: 1, max: 5 },
            Err(ReaderError::OutOfSchema(SchemaFault::OutOfRange)),
        ),
        (
            "an integer that is text",
            r#"{"answer": "4"}"#,
            ValueSchema::Integer { min: 1, max: 5 },
            Err(ReaderError::OutOfSchema(SchemaFault::WrongType)),
        ),
        (
            "a date",
            r#"{"answer": "2026-10-03"}"#,
            ValueSchema::Date,
            Ok(Value::Date(CivilDate {
                year: 2026,
                month: 10,
                day: 3,
            })),
        ),
        (
            "a date that is words",
            r#"{"answer": "next friday"}"#,
            ValueSchema::Date,
            Err(ReaderError::OutOfSchema(SchemaFault::WrongType)),
        ),
        (
            "an instant",
            r#"{"answer": "1970-01-02T00:00:00Z"}"#,
            ValueSchema::DateTime,
            Ok(Value::DateTime(prov::UnixSeconds(86_400))),
        ),
        (
            "a short text",
            r#"{"answer": "Booked"}"#,
            ValueSchema::Text { max: CharCount(10) },
            Ok(Value::Text("Booked".into())),
        ),
        (
            "a text that runs on",
            r#"{"answer": "Booked and then some more"}"#,
            ValueSchema::Text { max: CharCount(10) },
            Err(ReaderError::OutOfSchema(SchemaFault::TooLong)),
        ),
        (
            "entities the ask offered",
            r#"{"answer": ["org.quire.Mail/mail.thread/t2"]}"#,
            among.clone(),
            Ok(Value::Entities(vec![thread("t2")])),
        ),
        (
            "an entity it did not",
            r#"{"answer": ["org.quire.Mail/mail.thread/t9"]}"#,
            among,
            Err(ReaderError::OutOfSchema(SchemaFault::NotInSet)),
        ),
        (
            "a record",
            r#"{"total": 120, "when": "2026-10-03", "kind": "hotel"}"#,
            record(),
            Ok(Value::Record(BTreeMap::from([
                (p("total"), Value::Integer(120)),
                (
                    p("when"),
                    Value::Date(CivilDate {
                        year: 2026,
                        month: 10,
                        day: 3,
                    }),
                ),
                (p("kind"), Value::Choice(c("hotel"))),
            ]))),
        ),
        (
            "a record with a field missing",
            r#"{"total": 120, "kind": "hotel"}"#,
            record(),
            Err(ReaderError::OutOfSchema(SchemaFault::MissingField(p(
                "when",
            )))),
        ),
        (
            "a record with a field of its own",
            r#"{"total": 1, "when": "2026-10-03", "kind": "hotel", "forward_to": "eve@evil.test"}"#,
            record(),
            Err(ReaderError::OutOfSchema(SchemaFault::UnknownField(p(
                "forward_to",
            )))),
        ),
        (
            "a list",
            r#"{"answer": [1, 2]}"#,
            ValueSchema::List {
                of: Box::new(ValueSchema::Integer { min: 0, max: 9 }),
                max: porter_core::Count(3),
            },
            Ok(Value::List(vec![Value::Integer(1), Value::Integer(2)])),
        ),
        (
            "a list that is too long",
            r#"{"answer": [1, 2, 3, 4]}"#,
            ValueSchema::List {
                of: Box::new(ValueSchema::Integer { min: 0, max: 9 }),
                max: porter_core::Count(3),
            },
            Err(ReaderError::OutOfSchema(SchemaFault::TooMany)),
        ),
        (
            "prose around the JSON",
            "Sure! {\"answer\": 3}",
            ValueSchema::Integer { min: 1, max: 5 },
            Err(ReaderError::Unparseable),
        ),
        (
            "no answer in the object",
            r#"{"result": 3}"#,
            ValueSchema::Integer { min: 1, max: 5 },
            Err(ReaderError::Unparseable),
        ),
        (
            "nothing",
            "",
            ValueSchema::Integer { min: 1, max: 5 },
            Err(ReaderError::Unparseable),
        ),
    ];
    for (name, reply, schema, want) in rows {
        assert_eq!(answer_of(reply, &schema), want, "{name}");
    }
}
