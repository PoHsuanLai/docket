//! JSON arguments to typed ones: one row per declared type, and every way to be wrong.

use actions_mcp::*;
use docket_core::*;
use porter_core::AppName;
use prov::{ClientName, Effect, EntityId, EntityKey, EntityKind, Integrity, Label, Source};
use serde_json::{Value as Json, json};

fn label() -> Label {
    mcp_label(&ClientName::parse("Claude Desktop").expect("client"))
}

fn param(name: &str, ty: ParamType, need: ParamNeed) -> ParamDecl {
    ParamDecl {
        name: ParamName::parse(name).expect("name"),
        label: LabelText::parse("a parameter").expect("label"),
        ty,
        need,
        sink: ArgSink::Inert,
    }
}

fn action(on: TargetKind, params: Vec<ParamDecl>) -> ActionDecl {
    ActionDecl {
        name: prov::ActionName::parse("mail.test.run").expect("action"),
        label: LabelText::parse("Run").expect("label"),
        on,
        params,
        effect: Effect::Read,
        classes: Default::default(),
        undo: UndoSupport::NotUndoable,
        reach: AgentReach::Offered,
        latency: Latency::Instant,
        result: ResultShape::Nothing,
        keys: KeyHint::None,
        lasting: Lasting::No,
        dry_run: DryRun::None,
        per_call: PerCall::Declared,
    }
}

fn kind(k: &str) -> EntityKind {
    EntityKind::parse(k).expect("kind")
}

fn thread(key: &str) -> EntityId {
    EntityId {
        app: AppName::parse("org.quire.Mail").expect("app"),
        kind: kind("mail.thread"),
        key: EntityKey::parse(key).expect("key"),
    }
}

fn entity_json(key: &str) -> Json {
    json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": key })
}

fn one(ty: ParamType, given: Json) -> Result<Value, ArgsFault> {
    let decl = action(
        TargetKind::Nothing,
        vec![param("p", ty, ParamNeed::Required)],
    );
    let map = json!({ "p": given });
    let (_, args) = read_call(&decl, map.as_object(), &label())?;
    let name = ParamName::parse("p").expect("p");
    Ok(args[&name].value.clone())
}

fn text(max: u32, lines: Lines) -> ParamType {
    ParamType::Text {
        max: CharCount(max),
        lines,
    }
}

fn wrong(why: Why) -> ArgsFault {
    ArgsFault::Wrong {
        param: ParamName::parse("p").expect("p"),
        why,
    }
}

#[test]
fn each_declared_type_reads_its_json() {
    let choice = ParamType::Choice(vec![ChoiceDecl {
        id: ChoiceId::parse("research").expect("id"),
        label: LabelText::parse("Research").expect("label"),
    }]);
    let rows: Vec<(ParamType, Json, Value)> = vec![
        (
            text(5, Lines::One),
            json!("hello"),
            Value::Text("hello".into()),
        ),
        (
            text(20, Lines::Many),
            json!("a\nb"),
            Value::Text("a\nb".into()),
        ),
        (
            ParamType::Integer { min: 1, max: 9 },
            json!(9),
            Value::Integer(9),
        ),
        (
            ParamType::Decimal { scale: Scale(3) },
            json!({ "units": 1250, "scale": 3 }),
            Value::Decimal(Decimal {
                units: 1250,
                scale: Scale(3),
            }),
        ),
        (
            ParamType::Date,
            json!({ "year": 2026, "month": 10, "day": 3 }),
            Value::Date(CivilDate {
                year: 2026,
                month: 10,
                day: 3,
            }),
        ),
        (
            ParamType::DateTime,
            json!(1_700_000_000),
            Value::DateTime(prov::UnixSeconds(1_700_000_000)),
        ),
        (ParamType::Duration, json!(90), Value::Duration(Seconds(90))),
        (
            choice,
            json!("research"),
            Value::Choice(ChoiceId::parse("research").expect("id")),
        ),
        (
            ParamType::Entity(kind("mail.thread")),
            entity_json("t1"),
            Value::Entity(thread("t1")),
        ),
        (
            ParamType::Entities(kind("mail.thread")),
            json!([entity_json("t1"), entity_json("t2")]),
            Value::Entities(vec![thread("t1"), thread("t2")]),
        ),
        (
            ParamType::File,
            json!("/home/p/a.txt"),
            Value::File(FileRef::parse("/home/p/a.txt").expect("file")),
        ),
        (
            ParamType::Url,
            json!("https://example.org/x"),
            Value::Url("https://example.org/x".into()),
        ),
        (
            ParamType::Dynamic(prov::ActionName::parse("mail.contact.suggest").expect("a")),
            json!("accounting"),
            Value::Text("accounting".into()),
        ),
    ];
    for (ty, given, want) in rows {
        assert_eq!(one(ty.clone(), given.clone()), Ok(want), "{ty:?} {given}");
    }
}

#[test]
fn a_handle_stands_in_where_the_schema_allows_one() {
    let held = json!({ "handle": 4 });
    for ty in [
        text(9, Lines::One),
        ParamType::Entity(kind("mail.thread")),
        ParamType::File,
        ParamType::Url,
    ] {
        assert_eq!(
            one(ty.clone(), held.clone()),
            Ok(Value::Handle(Handle(4))),
            "{ty:?}"
        );
    }
    for ty in [
        ParamType::Integer { min: 0, max: 9 },
        ParamType::Entities(kind("mail.thread")),
        ParamType::Date,
    ] {
        assert_eq!(
            one(ty.clone(), held.clone()),
            Err(wrong(Why::Type)),
            "{ty:?}"
        );
    }
    let extra = json!({ "handle": 4, "also": 1 });
    assert_eq!(one(text(9, Lines::One), extra), Err(wrong(Why::Type)));
}

#[test]
fn a_value_that_does_not_fit_is_refused_with_its_parameter() {
    let rows: Vec<(ParamType, Json, Why)> = vec![
        (text(3, Lines::One), json!("long"), Why::Range),
        (text(9, Lines::One), json!("a\nb"), Why::Range),
        (text(9, Lines::One), json!(4), Why::Type),
        (ParamType::Integer { min: 1, max: 9 }, json!(10), Why::Range),
        (ParamType::Integer { min: 1, max: 9 }, json!("3"), Why::Type),
        (ParamType::Integer { min: 1, max: 9 }, json!(2.5), Why::Type),
        (
            ParamType::Decimal { scale: Scale(3) },
            json!({ "units": 1, "scale": 2 }),
            Why::Range,
        ),
        (
            ParamType::Date,
            json!({ "year": 2026, "month": 13, "day": 3 }),
            Why::Range,
        ),
        (ParamType::Date, json!("2026-10-03"), Why::Type),
        (ParamType::Duration, json!(-1), Why::Range),
        (ParamType::Choice(vec![]), json!("anything"), Why::Range),
        (
            ParamType::Entity(kind("mail.thread")),
            json!({ "app": "org.quire.Mail", "kind": "mail.contact", "key": "c1" }),
            Why::Range,
        ),
        (
            ParamType::Entity(kind("mail.thread")),
            json!("t1"),
            Why::Type,
        ),
        (ParamType::Url, json!("not a link"), Why::Range),
        (ParamType::File, json!(""), Why::Range),
    ];
    for (ty, given, why) in rows {
        assert_eq!(
            one(ty.clone(), given.clone()),
            Err(wrong(why)),
            "{ty:?} {given}"
        );
    }
}

#[test]
fn every_argument_is_labelled_untrusted_from_this_client() {
    let decl = action(
        TargetKind::Nothing,
        vec![
            param("a", text(9, Lines::One), ParamNeed::Required),
            param(
                "b",
                ParamType::Integer { min: 0, max: 9 },
                ParamNeed::Required,
            ),
        ],
    );
    let given = json!({ "a": "x", "b": 2 });
    let (_, args) = read_call(&decl, given.as_object(), &label()).expect("args");
    assert_eq!(args.len(), 2);
    for held in args.values() {
        assert_eq!(held.label.integrity, Integrity::Untrusted);
        assert!(matches!(
            held.label.sources.iter().next(),
            Some(Source::Mcp(client)) if client.as_str() == "Claude Desktop"
        ));
    }
}

#[test]
fn missing_unknown_and_optional_arguments() {
    let decl = action(
        TargetKind::Nothing,
        vec![
            param("need", text(9, Lines::One), ParamNeed::Required),
            param("maybe", text(9, Lines::One), ParamNeed::Optional),
        ],
    );
    let read = |given: Json| read_call(&decl, given.as_object(), &label()).map(|(_, a)| a.len());
    assert_eq!(read(json!({ "need": "x" })), Ok(1));
    assert_eq!(read(json!({ "need": "x", "maybe": "y" })), Ok(2));
    assert_eq!(
        read(json!({})),
        Err(ArgsFault::Missing(ParamName::parse("need").expect("p")))
    );
    assert_eq!(
        read(json!({ "need": "x", "extra": 1 })),
        Err(ArgsFault::Unknown("extra".into()))
    );
    assert_eq!(
        read_call(&decl, None, &label()).map(|_| ()),
        Err(ArgsFault::Missing(ParamName::parse("need").expect("p")))
    );
}

#[test]
fn the_target_follows_the_actions_on() {
    let t = |on: TargetKind, given: Option<Json>| {
        let decl = action(on, vec![]);
        let map = given.map(|g| json!({ "target": g }));
        read_call(&decl, map.as_ref().and_then(Json::as_object), &label()).map(|(t, _)| t)
    };
    let thread_kind = kind("mail.thread");
    assert_eq!(t(TargetKind::Nothing, None), Ok(TargetValue::Nothing));
    assert_eq!(
        t(TargetKind::Nothing, Some(json!("x"))),
        Err(ArgsFault::Target(TargetFault::Unexpected))
    );
    assert_eq!(
        t(
            TargetKind::One(thread_kind.clone()),
            Some(entity_json("t1"))
        ),
        Ok(TargetValue::Entities(vec![thread("t1")]))
    );
    assert_eq!(
        t(TargetKind::One(thread_kind.clone()), None),
        Err(ArgsFault::Target(TargetFault::Missing))
    );
    assert_eq!(
        t(
            TargetKind::Many(thread_kind.clone()),
            Some(json!([entity_json("t1"), entity_json("t2")]))
        ),
        Ok(TargetValue::Entities(vec![thread("t1"), thread("t2")]))
    );
    assert_eq!(
        t(TargetKind::Many(thread_kind.clone()), Some(json!([]))),
        Err(ArgsFault::Target(TargetFault::Malformed))
    );
    assert_eq!(
        t(TargetKind::One(thread_kind), Some(json!({ "key": "t1" }))),
        Err(ArgsFault::Target(TargetFault::Malformed))
    );
    assert_eq!(
        t(TargetKind::Files, Some(json!(["/a/b"]))),
        Ok(TargetValue::Files(vec![FileRef::parse("/a/b").expect("f")]))
    );
    assert_eq!(
        t(TargetKind::Text, Some(json!("field"))),
        Err(ArgsFault::Target(TargetFault::NotNameable))
    );
}
