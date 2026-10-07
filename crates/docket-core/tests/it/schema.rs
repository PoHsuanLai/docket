//! Reader schemas and the arguments of the task-start action.

use crate::support::*;
use docket_core::*;
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

fn args(items: Vec<(&str, Value)>) -> Args {
    items
        .into_iter()
        .map(|(k, v)| (param(k), lab(v, trusted())))
        .collect()
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
fn task_start_reads_its_arguments() {
    let research = ChoiceId::parse("research").expect("id");
    type Case = (
        &'static str,
        Args,
        Result<(&'static str, TaskKind), ArgFault>,
    );
    let cases: Vec<Case> = vec![
        (
            "goal and kind",
            args(vec![
                ("goal", Value::Text("find the Lisbon receipts".into())),
                ("kind", Value::Choice(ChoiceId::parse("watch").expect("id"))),
            ]),
            Ok(("find the Lisbon receipts", TaskKind::Watch)),
        ),
        (
            "kind defaults to research",
            args(vec![("goal", Value::Text("look up".into()))]),
            Ok(("look up", TaskKind::Research)),
        ),
        (
            "an empty goal is missing",
            args(vec![("goal", Value::Text(String::new()))]),
            Err(ArgFault::Missing),
        ),
        (
            "no goal is missing",
            args(vec![("kind", Value::Choice(research))]),
            Err(ArgFault::Missing),
        ),
        (
            "a goal that is not text",
            args(vec![("goal", Value::Integer(3))]),
            Err(ArgFault::WrongType),
        ),
        (
            "a kind outside the choices",
            args(vec![
                ("goal", Value::Text("x".into())),
                ("kind", Value::Choice(ChoiceId::parse("rule").expect("id"))),
            ]),
            Err(ArgFault::OutOfRange),
        ),
    ];
    for (name, a, want) in cases {
        let got = TaskStart::from_args(&a).map(|t| (t.goal.as_str().to_owned(), t.kind));
        assert_eq!(got, want.map(|(g, k)| (g.to_owned(), k)), "case: {name}");
    }
}
