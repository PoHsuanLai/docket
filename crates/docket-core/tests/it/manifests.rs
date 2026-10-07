//! The shipped manifests parse and validate, and every `ManifestError` rule has its row.

use crate::support::*;
use docket_core::*;
use prov::Effect;
use std::path::PathBuf;

fn shipped(name: &str) -> Manifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../manifests")
        .join(name);
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    toml::from_str(&text).unwrap_or_else(|e| panic!("{name}: {e}"))
}

#[test]
fn shipped_manifests_parse_and_validate() {
    for (file, prefix, actions) in [
        ("org.quire.Memory.toml", "memory", 4),
        ("org.quire.Companion.toml", "companion", 3),
    ] {
        let manifest = shipped(file);
        assert_eq!(manifest.actions.len(), actions, "{file}");
        let valid = validate(manifest.clone()).unwrap_or_else(|e| panic!("{file}: {e}"));
        assert!(
            valid
                .manifest()
                .actions
                .iter()
                .all(|a| a.name.as_str().starts_with(prefix))
        );
        // The serde form is the file format: a round trip through JSON changes nothing.
        let json = serde_json::to_string(&valid).expect("json");
        let back: ValidManifest = serde_json::from_str(&json).expect("valid manifest");
        assert_eq!(back, valid);
    }
}

#[test]
fn memory_forget_is_hidden_from_agents_and_staged_writes_are_declared() {
    let memory = shipped("org.quire.Memory.toml");
    let by = |n: &str| {
        memory
            .actions
            .iter()
            .find(|a| a.name.as_str() == n)
            .expect(n)
    };
    assert_eq!(by("memory.forget").reach, AgentReach::Hidden);
    assert_eq!(by("memory.forget").effect, Effect::Destructive);
    assert_eq!(by("memory.propose").lasting, Lasting::Staged);
    assert_eq!(by("memory.recall").effect, Effect::Read);
}

#[test]
fn companion_task_actions_only_read() {
    let companion = shipped("org.quire.Companion.toml");
    assert!(companion.actions.iter().all(|a| a.effect == Effect::Read));
}

#[test]
fn validate_rejects_one_row_per_rule() {
    use ManifestError as E;
    let recipient = text_param("to", ArgSink::Recipient, ParamNeed::Required);
    let mut outbound = decl(
        "mail.message.send",
        Effect::Outbound,
        UndoSupport::NotUndoable,
    );
    outbound.params = vec![recipient.clone()];
    let mut bad_default = decl("mail.thread.tag", Effect::UndoableWrite, UndoSupport::Token);
    bad_default.params = vec![text_param(
        "tag",
        ArgSink::Inert,
        ParamNeed::Defaulted(Value::Integer(3)),
    )];
    let mut twice = decl("mail.thread.tag2", Effect::Read, UndoSupport::NotUndoable);
    twice.params = vec![
        text_param("x", ArgSink::Inert, ParamNeed::Optional),
        text_param("x", ArgSink::Inert, ParamNeed::Optional),
    ];
    let mut no_sink = decl("mail.message.forward", Effect::Outbound, UndoSupport::Token);
    no_sink.params = vec![text_param("note", ArgSink::Body, ParamNeed::Optional)];

    let mut newer = manifest("org.quire.Mail", vec![]);
    newer.vocab = IntentsVocab(IntentsVocab::CURRENT.0 + 1);

    let cases: Vec<(&str, Manifest, Option<ManifestError>)> = vec![
        (
            "ok: outbound with a recipient and no undo",
            manifest("org.quire.Mail", vec![outbound.clone()]),
            None,
        ),
        (
            "ok: outbound may hold the send and declare a token",
            {
                let mut held = outbound.clone();
                held.undo = UndoSupport::Token;
                manifest("org.quire.Mail", vec![held])
            },
            None,
        ),
        (
            "vocab too new",
            newer,
            Some(E::VocabTooNew(IntentsVocab(2))),
        ),
        (
            "duplicate action",
            manifest("org.quire.Mail", vec![outbound.clone(), outbound.clone()]),
            Some(E::DuplicateAction(action("mail.message.send"))),
        ),
        (
            "read with undo",
            manifest(
                "org.quire.Mail",
                vec![decl("mail.thread.list", Effect::Read, UndoSupport::Token)],
            ),
            Some(E::ReadWithUndo(action("mail.thread.list"))),
        ),
        (
            "write without undo",
            manifest(
                "org.quire.Mail",
                vec![decl(
                    "mail.thread.tag",
                    Effect::UndoableWrite,
                    UndoSupport::NotUndoable,
                )],
            ),
            Some(E::WriteWithoutUndo(action("mail.thread.tag"))),
        ),
        (
            "destructive without undo is fine",
            manifest(
                "org.quire.Mail",
                vec![decl(
                    "mail.thread.delete",
                    Effect::Destructive,
                    UndoSupport::NotUndoable,
                )],
            ),
            None,
        ),
        (
            "outside the app's prefix",
            manifest(
                "org.quire.Mail",
                vec![decl(
                    "files.thing.read",
                    Effect::Read,
                    UndoSupport::NotUndoable,
                )],
            ),
            Some(E::ActionOutsideApp(action("files.thing.read"))),
        ),
        (
            "default out of type",
            manifest("org.quire.Mail", vec![bad_default]),
            Some(E::DefaultOutOfType {
                action: action("mail.thread.tag"),
                param: param("tag"),
            }),
        ),
        (
            "duplicate param",
            manifest("org.quire.Mail", vec![twice]),
            Some(E::DuplicateParam {
                action: action("mail.thread.tag2"),
                param: param("x"),
            }),
        ),
        (
            "outbound without a sink",
            manifest("org.quire.Mail", vec![no_sink]),
            Some(E::OutboundWithoutSink(action("mail.message.forward"))),
        ),
    ];
    for (name, manifest, want) in cases {
        let got = validate(manifest).err();
        assert_eq!(got, want, "case: {name}");
    }
}

fn thread_kind(index: IndexPolicy) -> EntityDecl {
    EntityDecl {
        kind: kind("mail.thread"),
        label: words("Thread"),
        plural: words("Threads"),
        icon: IconName::parse("mail").expect("icon"),
        class: prov::DataClass::Mail,
        index,
        titles: TitleTrust::AppAuthored,
        props: vec![],
    }
}

fn open_decl() -> ActionDecl {
    let mut open = decl("mail.thread.open", Effect::Read, UndoSupport::NotUndoable);
    open.on = TargetKind::One(kind("mail.thread"));
    open
}

#[test]
fn an_indexed_kind_declares_its_open_action_and_every_open_has_the_one_shape() {
    use ManifestError as E;
    let with = |index, actions| {
        let mut m = manifest("org.quire.Mail", actions);
        m.entities = vec![thread_kind(index)];
        m
    };
    let mut on_many = open_decl();
    on_many.on = TargetKind::Many(kind("mail.thread"));
    let mut writes = open_decl();
    writes.effect = Effect::UndoableWrite;
    writes.undo = UndoSupport::Token;
    let mut asks = open_decl();
    asks.params = vec![text_param("where", ArgSink::Inert, ParamNeed::Required)];
    let mut optional = open_decl();
    optional.params = vec![text_param("where", ArgSink::Inert, ParamNeed::Optional)];
    let bad = |a: &ActionDecl| Some(E::BadOpen(a.name.clone()));
    let cases: Vec<(&str, Manifest, Option<ManifestError>)> = vec![
        (
            "indexed with its open",
            with(IndexPolicy::Indexed, vec![open_decl()]),
            None,
        ),
        (
            "indexed without one",
            with(IndexPolicy::Indexed, vec![]),
            Some(E::OpenMissing(kind("mail.thread"))),
        ),
        (
            "live-searched kinds may omit it",
            with(IndexPolicy::NotIndexed, vec![]),
            None,
        ),
        (
            "an optional parameter is fine",
            with(IndexPolicy::Indexed, vec![optional]),
            None,
        ),
        (
            "on many",
            with(IndexPolicy::Indexed, vec![on_many.clone()]),
            bad(&on_many),
        ),
        (
            "writes",
            with(IndexPolicy::NotIndexed, vec![writes.clone()]),
            bad(&writes),
        ),
        (
            "requires a parameter",
            with(IndexPolicy::Indexed, vec![asks.clone()]),
            bad(&asks),
        ),
    ];
    for (name, manifest, want) in cases {
        assert_eq!(validate(manifest).err(), want, "case: {name}");
    }
}

#[test]
fn defaults_must_fit_their_type() {
    let cases: Vec<(&str, Value, ParamType, bool)> = vec![
        (
            "short text",
            Value::Text("hi".into()),
            ParamType::Text {
                max: CharCount(5),
                lines: Lines::One,
            },
            true,
        ),
        (
            "long text",
            Value::Text("toolong".into()),
            ParamType::Text {
                max: CharCount(5),
                lines: Lines::One,
            },
            false,
        ),
        (
            "newline in a one-line field",
            Value::Text("a\nb".into()),
            ParamType::Text {
                max: CharCount(5),
                lines: Lines::One,
            },
            false,
        ),
        (
            "newline in a many-line field",
            Value::Text("a\nb".into()),
            ParamType::Text {
                max: CharCount(5),
                lines: Lines::Many,
            },
            true,
        ),
        (
            "integer in range",
            Value::Integer(5),
            ParamType::Integer { min: 1, max: 5 },
            true,
        ),
        (
            "integer out of range",
            Value::Integer(6),
            ParamType::Integer { min: 1, max: 5 },
            false,
        ),
        (
            "wrong kind of value",
            Value::Integer(1),
            ParamType::Date,
            false,
        ),
        (
            "entity of the kind",
            Value::Entity(entity("mail.thread", "a")),
            ParamType::Entity(kind("mail.thread")),
            true,
        ),
        (
            "entity of another kind",
            Value::Entity(entity("mail.draft", "a")),
            ParamType::Entity(kind("mail.thread")),
            false,
        ),
    ];
    for (name, value, ty, ok) in cases {
        assert_eq!(fits(&value, &ty), ok, "case: {name}");
    }
}
