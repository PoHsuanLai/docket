//! How `--param value` words map onto the manifest's declared types (cli.md section 4): a table
//! with one row per type, the words that fail, and the command line's own grammar.

use docket_cli::params::{Reading, Stdin, StdinSlot, arguments, target};
use docket_cli::resolve::Apps;
use docket_core::{ActionDecl, CivilDate, Decimal, Handle, Scale, Seconds, TargetValue, Value};
use docket_router::parse as parse_manifest;
use prov::{EntityId, EntityKey, EntityKind, UnixSeconds};

const DEMO: &str = r#"
vocab = 1
app = "org.quire.Demo"

[[entities]]
kind = "demo.item"
label = "Item"
plural = "Items"
icon = "demo"
class = "app_own"
index = "not_indexed"
titles = { kind = "app_authored" }
props = []

[[actions]]
name = "demo.item.tune"
label = "Tune"
on = { kind = "many", v = "demo.item" }
effect = "undoable_write"
classes = ["app_own"]
undo = "token"
reach = "offered"
latency = "instant"
result = { kind = "nothing" }
keys = { kind = "none" }
lasting = "no"
dry_run = "none"

[[actions.params]]
name = "title"
label = "Title"
ty = { kind = "text", v = { max = 20, lines = "one" } }
need = { kind = "required" }
sink = "inert"

[[actions.params]]
name = "notes"
label = "Notes"
ty = { kind = "text", v = { max = 100, lines = "many" } }
need = { kind = "optional" }
sink = "body"

[[actions.params]]
name = "volume"
label = "Volume"
ty = { kind = "integer", v = { min = 0, max = 11 } }
need = { kind = "optional" }
sink = "inert"

[[actions.params]]
name = "price"
label = "Price"
ty = { kind = "decimal", v = { scale = 2 } }
need = { kind = "optional" }
sink = "inert"

[[actions.params]]
name = "due"
label = "Due"
ty = { kind = "date" }
need = { kind = "optional" }
sink = "inert"

[[actions.params]]
name = "at"
label = "At"
ty = { kind = "date_time" }
need = { kind = "optional" }
sink = "inert"

[[actions.params]]
name = "every"
label = "Every"
ty = { kind = "duration" }
need = { kind = "optional" }
sink = "inert"

[[actions.params]]
name = "mood"
label = "Mood"
ty = { kind = "choice", v = [{ id = "calm", label = "Calm" }, { id = "loud", label = "Loud" }] }
need = { kind = "optional" }
sink = "inert"

[[actions.params]]
name = "parent"
label = "Parent"
ty = { kind = "entity", v = "demo.item" }
need = { kind = "optional" }
sink = "inert"

[[actions.params]]
name = "friend"
label = "Friend"
ty = { kind = "entity", v = "other.thing" }
need = { kind = "optional" }
sink = "inert"

[[actions.params]]
name = "peers"
label = "Peers"
ty = { kind = "entities", v = "demo.item" }
need = { kind = "optional" }
sink = "inert"

[[actions.params]]
name = "source"
label = "Source"
ty = { kind = "file" }
need = { kind = "optional" }
sink = "path"

[[actions.params]]
name = "link"
label = "Link"
ty = { kind = "url" }
need = { kind = "optional" }
sink = "destination"
"#;

const OTHER: &str = r#"
vocab = 1
app = "org.quire.Other"
actions = []

[[entities]]
kind = "other.thing"
label = "Thing"
plural = "Things"
icon = "other"
class = "app_own"
index = "not_indexed"
titles = { kind = "app_authored" }
props = []
"#;

fn apps() -> Apps {
    Apps::new(vec![
        parse_manifest(DEMO).expect("demo"),
        parse_manifest(OTHER).expect("other"),
    ])
}

fn demo(apps: &Apps) -> (&docket_core::Manifest, &ActionDecl) {
    let app = apps.app("demo").expect("app");
    let action = apps.action(app, "item.tune").expect("action");
    (app, action)
}

fn given(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect()
}

fn map(pairs: &[(&str, &str)], stdin: Stdin) -> Result<docket_core::Args, String> {
    let apps = apps();
    let (app, action) = demo(&apps);
    let mut slot = StdinSlot::new(stdin);
    let mut reading = Reading {
        apps: &apps,
        owner: &app.app,
        stdin: &mut slot,
    };
    arguments(action, &mut reading, &given(pairs)).map_err(|f| f.what)
}

fn item(key: &str) -> EntityId {
    EntityId {
        app: porter_core::AppName::parse("org.quire.Demo").expect("app"),
        kind: EntityKind::parse("demo.item").expect("kind"),
        key: EntityKey::parse(key).expect("key"),
    }
}

#[test]
fn each_declared_type_maps_from_its_words() {
    let one = |name: &str, raw: &str| -> Value {
        let pairs: Vec<(&str, &str)> = if name == "title" {
            vec![("title", raw)]
        } else {
            vec![("title", "t"), (name, raw)]
        };
        let args = map(&pairs, Stdin::Closed).unwrap_or_else(|e| panic!("{name}={raw}: {e}"));
        let param = docket_core::ParamName::parse(name).expect("param");
        args.get(&param).expect("the argument").value.clone()
    };
    let rows: Vec<(&str, &str, Value)> = vec![
        ("title", "Hello", Value::Text("Hello".into())),
        ("volume", "11", Value::Integer(11)),
        ("volume", "0", Value::Integer(0)),
        (
            "price",
            "12.5",
            Value::Decimal(Decimal {
                units: 1250,
                scale: Scale(2),
            }),
        ),
        (
            "price",
            "-3",
            Value::Decimal(Decimal {
                units: -300,
                scale: Scale(2),
            }),
        ),
        (
            "due",
            "2026-10-03",
            Value::Date(CivilDate {
                year: 2026,
                month: 10,
                day: 3,
            }),
        ),
        (
            "due",
            "2026-02-28T09:30:00+02:00",
            Value::Date(CivilDate {
                year: 2026,
                month: 2,
                day: 28,
            }),
        ),
        (
            "at",
            "1970-01-01T00:00:00Z",
            Value::DateTime(UnixSeconds(0)),
        ),
        (
            "at",
            "2026-10-03T09:30:00+02:00",
            Value::DateTime(UnixSeconds(1_791_012_600)),
        ),
        (
            "at",
            "2026-10-03",
            Value::DateTime(UnixSeconds(1_790_985_600)),
        ),
        ("every", "90", Value::Duration(Seconds(90))),
        ("every", "1h30m", Value::Duration(Seconds(5400))),
        ("every", "2d", Value::Duration(Seconds(172_800))),
        (
            "mood",
            "loud",
            Value::Choice(docket_core::ChoiceId::parse("loud").expect("choice")),
        ),
        ("parent", "demo.item:a1", Value::Entity(item("a1"))),
        ("parent", "a1", Value::Entity(item("a1"))),
        ("parent", "#7", Value::Handle(Handle(7))),
        (
            "friend",
            "other.thing:z",
            Value::Entity(EntityId {
                app: porter_core::AppName::parse("org.quire.Other").expect("app"),
                kind: EntityKind::parse("other.thing").expect("kind"),
                key: EntityKey::parse("z").expect("key"),
            }),
        ),
        (
            "link",
            "https://example.org/a",
            Value::Url("https://example.org/a".into()),
        ),
        ("title", "#3", Value::Handle(Handle(3))),
    ];
    for (name, raw, want) in rows {
        assert_eq!(one(name, raw), want, "--{name} {raw}");
    }
    // A file is the absolute path it names.
    let file = one("source", "/tmp/a.txt");
    assert_eq!(
        file,
        Value::File(docket_core::FileRef::parse("/tmp/a.txt").expect("file"))
    );
    let relative = one("source", "notes.txt");
    assert!(
        matches!(&relative, Value::File(f) if f.as_str().starts_with('/') && f.as_str().ends_with("notes.txt")),
        "{relative:?}"
    );
}

#[test]
fn several_things_are_a_repeated_flag() {
    let args = map(
        &[("title", "t"), ("peers", "demo.item:a"), ("peers", "b")],
        Stdin::Closed,
    )
    .expect("mapped");
    let peers = docket_core::ParamName::parse("peers").expect("param");
    assert_eq!(
        args[&peers].value,
        Value::Entities(vec![item("a"), item("b")])
    );
}

#[test]
fn words_that_do_not_fit_are_usage_errors_that_say_why() {
    let rows: [(&[(&str, &str)], &str); 14] = [
        (&[], "--title is required"),
        (
            &[("title", "a very long title that does not fit")],
            "longer than 20",
        ),
        (&[("title", "two\nlines")], "one line"),
        (&[("title", "t"), ("volume", "12")], "from 0 to 11"),
        (&[("title", "t"), ("volume", "loud")], "whole number"),
        (&[("title", "t"), ("price", "1.234")], "at most 2 digits"),
        (&[("title", "t"), ("due", "tomorrow")], "RFC 3339"),
        (&[("title", "t"), ("due", "2026-02-30")], "RFC 3339"),
        (
            &[("title", "t"), ("at", "2026-10-03T25:00:00Z")],
            "RFC 3339",
        ),
        (&[("title", "t"), ("every", "soon")], "seconds"),
        (&[("title", "t"), ("mood", "grumpy")], "one of: calm, loud"),
        (
            &[("title", "t"), ("parent", "other.thing:z")],
            "a demo.item is wanted",
        ),
        (&[("title", "t"), ("link", "example.org")], "link"),
        (&[("title", "t"), ("bogus", "x")], "not a parameter"),
    ];
    for (pairs, why) in rows {
        let err = map(pairs, Stdin::Closed).expect_err(why);
        assert!(err.contains(why), "{pairs:?}: {err}");
    }
    let twice = map(&[("title", "a"), ("title", "b")], Stdin::Closed).expect_err("twice");
    assert!(twice.contains("given twice"), "{twice}");
    let list = map(&[("title", "t"), ("peers", "#3")], Stdin::Closed).expect_err("handle in list");
    assert!(list.contains("held handle"), "{list}");
}

#[test]
fn a_dash_reads_standard_input_once() {
    let args = map(&[("title", "-")], Stdin::Text("from a pipe\n".into())).expect("mapped");
    let title = docket_core::ParamName::parse("title").expect("param");
    assert_eq!(
        args[&title].value,
        Value::Text("from a pipe".into()),
        "one trailing newline goes"
    );
    let many = map(
        &[("title", "t"), ("notes", "-")],
        Stdin::Text("a\nb\n".into()),
    )
    .expect("mapped");
    let notes = docket_core::ParamName::parse("notes").expect("param");
    assert_eq!(
        many[&notes].value,
        Value::Text("a\nb".into()),
        "lines stay in a many-lines text"
    );
    let none = map(&[("title", "-")], Stdin::Closed).expect_err("closed");
    assert!(none.contains("nothing was piped"), "{none}");
    let twice = map(&[("title", "-"), ("notes", "-")], Stdin::Text("x".into())).expect_err("twice");
    assert!(twice.contains("once"), "{twice}");
}

#[test]
fn every_argument_is_labelled_untrusted_from_the_terminal() {
    let args = map(&[("title", "t")], Stdin::Closed).expect("mapped");
    for arg in args.values() {
        assert_eq!(arg.label.integrity, prov::Integrity::Untrusted);
        assert!(arg.label.sources.contains(&prov::Source::Cli));
    }
}

#[test]
fn targets_follow_the_declaration() {
    let apps = apps();
    let (app, action) = demo(&apps);
    let mut slot = StdinSlot::new(Stdin::Closed);
    let reading = Reading {
        apps: &apps,
        owner: &app.app,
        stdin: &mut slot,
    };
    let words = |w: &[&str]| w.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    assert_eq!(
        target(action, &reading, &words(&["demo.item:a", "b"])).expect("targets"),
        TargetValue::Entities(vec![item("a"), item("b")])
    );
    assert!(
        target(action, &reading, &[])
            .expect_err("none")
            .what
            .contains("at least one")
    );
    assert!(
        target(action, &reading, &words(&["other.thing:z"]))
            .expect_err("kind")
            .what
            .contains("wanted")
    );
}
