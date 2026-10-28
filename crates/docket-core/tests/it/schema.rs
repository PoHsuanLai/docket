//! Reader schemas.

use crate::support::*;
use docket_core::*;
use docket_vocab::{ActionRef, CallId, CharCount, ChoiceId, Value};
use porter_core::Count;

fn schema_cases() -> Vec<(&'static str, Value, ValueSchema, Result<(), SchemaFault>)> {
    let a = entity("mail.thread", "a");
    let b = entity("mail.thread", "b");
    let yes = ChoiceId::parse("yes").expect("id");
    let no = ChoiceId::parse("no").expect("id");
    let fields =
        |v: Vec<(&str, Value)>| Value::Record(v.into_iter().map(|(k, v)| (param(k), v)).collect());
    vec![
        (
            "choice in set",
            Value::Choice(yes.clone()),
            ValueSchema::Choice(vec![yes.clone(), no.clone()]),
            Ok(()),
        ),
        (
            "choice outside the set",
            Value::Choice(ChoiceId::parse("maybe").expect("id")),
            ValueSchema::Choice(vec![yes, no]),
            Err(SchemaFault::NotInSet),
        ),
        (
            "integer in range",
            Value::Integer(3),
            ValueSchema::Integer { min: 1, max: 3 },
            Ok(()),
        ),
        (
            "integer above range",
            Value::Integer(4),
            ValueSchema::Integer { min: 1, max: 3 },
            Err(SchemaFault::OutOfRange),
        ),
        (
            "entities among the offered",
            Value::Entities(vec![a.clone()]),
            ValueSchema::EntitiesAmong(vec![a.clone(), b.clone()]),
            Ok(()),
        ),
        (
            "an entity nobody offered",
            Value::Entities(vec![entity("mail.thread", "evil")]),
            ValueSchema::EntitiesAmong(vec![a.clone(), b.clone()]),
            Err(SchemaFault::NotInSet),
        ),
        (
            "text within the cap",
            Value::Text("ok".into()),
            ValueSchema::Text { max: CharCount(2) },
            Ok(()),
        ),
        (
            "text over the cap",
            Value::Text("too long".into()),
            ValueSchema::Text { max: CharCount(2) },
            Err(SchemaFault::TooLong),
        ),
        (
            "wrong type",
            Value::Integer(1),
            ValueSchema::Date,
            Err(SchemaFault::WrongType),
        ),
        (
            "record with every field",
            fields(vec![("n", Value::Integer(1))]),
            ValueSchema::Record(vec![(param("n"), ValueSchema::Integer { min: 0, max: 9 })]),
            Ok(()),
        ),
        (
            "record missing a field",
            fields(vec![]),
            ValueSchema::Record(vec![(param("n"), ValueSchema::Integer { min: 0, max: 9 })]),
            Err(SchemaFault::MissingField(param("n"))),
        ),
        (
            "record with an extra field",
            fields(vec![("n", Value::Integer(1)), ("x", Value::Integer(1))]),
            ValueSchema::Record(vec![(param("n"), ValueSchema::Integer { min: 0, max: 9 })]),
            Err(SchemaFault::UnknownField(param("x"))),
        ),
        (
            "list within its cap",
            Value::List(vec![Value::Integer(1)]),
            ValueSchema::List {
                of: Box::new(ValueSchema::Integer { min: 0, max: 9 }),
                max: Count(1),
            },
            Ok(()),
        ),
        (
            "list over its cap",
            Value::List(vec![Value::Integer(1), Value::Integer(2)]),
            ValueSchema::List {
                of: Box::new(ValueSchema::Integer { min: 0, max: 9 }),
                max: Count(1),
            },
            Err(SchemaFault::TooMany),
        ),
        (
            "list with a bad element",
            Value::List(vec![Value::Integer(10)]),
            ValueSchema::List {
                of: Box::new(ValueSchema::Integer { min: 0, max: 9 }),
                max: Count(3),
            },
            Err(SchemaFault::OutOfRange),
        ),
    ]
}

#[test]
fn conforms_table() {
    for (name, value, schema, want) in schema_cases() {
        assert_eq!(conforms(&value, &schema), want, "case: {name}");
    }
}

#[test]
fn value_schemas_render_as_shapes() {
    use model_provider::Shape;
    let schema = ValueSchema::Record(vec![
        (
            param("kind"),
            ValueSchema::Choice(vec![ChoiceId::parse("receipt").expect("id")]),
        ),
        (
            param("things"),
            ValueSchema::EntitiesAmong(vec![entity("mail.thread", "a")]),
        ),
    ]);
    let Shape::Record(fields) = schema.shape().expect("shape") else {
        panic!("a record")
    };
    assert_eq!(fields.len(), 2);
    assert_eq!(fields[0].name.as_str(), "kind");
    let Shape::List { of, max } = &fields[1].shape else {
        panic!("a list of choices")
    };
    assert_eq!(max.0, 1);
    let Shape::Choice(options) = of.as_ref() else {
        panic!("choices")
    };
    assert_eq!(options[0].0, "org.quire.Mail/mail.thread/a");
}

#[test]
fn the_readers_step_is_named_by_its_task_and_read_back_from_the_name() {
    let step = |app_name: &str, name: &str| StepLine {
        call: CallId(0),
        action: ActionRef {
            app: app(app_name),
            name: action(name),
        },
        effect: prov::Effect::Read,
        end: StepEnd::Interrupted,
        shown: StepShown::Full,
        with: Vec::new(),
    };
    for task in ReaderTask::ALL {
        let line = step("org.quire.Companion", &task.step_name());
        assert_eq!(line.reader_task(), Some(task));
    }
    assert_eq!(
        step("org.quire.Companion", "companion.read.guess").reader_task(),
        None
    );
    assert_eq!(
        step("org.quire.Mail", "companion.read.summarise").reader_task(),
        None,
        "only the companion's own app has a reader step"
    );
}
