//! The arguments of `companion.task.start`.

use crate::support::*;
use docket_core::ArgFault;
use docket_vocab::{Args, ChoiceId, Value};
use docket_wire::{TaskKind, TaskStart};

fn args(items: Vec<(&str, Value)>) -> Args {
    items
        .into_iter()
        .map(|(k, v)| (param(k), lab(v, trusted())))
        .collect()
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
