//! Builders the tests share; labels come from prov's constructors.

use docket_manifest::{
    ActionDecl, AgentReach, ArgSink, DryRun, KeyHint, Lasting, Latency, ParamDecl, ParamNeed,
    ResultShape, TargetKind, UndoSupport,
};
use docket_vocab::{CharCount, LabelText, Lines, ParamName, ParamType, PerCall};
use porter_core::{AppName, DataClass};
use prov::{
    ActionName, Effect, EntityId, EntityKey, EntityKind, Label, Labelled, Source, SpaceId,
    UnixSeconds,
};
use std::collections::BTreeSet;

pub fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}
pub fn space(name: &str) -> SpaceId {
    SpaceId::parse(name).expect("space")
}
pub fn action(name: &str) -> ActionName {
    ActionName::parse(name).expect("action")
}
pub fn kind(name: &str) -> EntityKind {
    EntityKind::parse(name).expect("kind")
}
pub fn param(name: &str) -> ParamName {
    ParamName::parse(name).expect("param")
}
pub fn words(text: &str) -> LabelText {
    LabelText::parse(text).expect("words")
}
pub fn entity(kind_name: &str, key: &str) -> EntityId {
    EntityId {
        app: app("org.quire.Mail"),
        kind: kind(kind_name),
        key: EntityKey::parse(key).expect("key"),
    }
}
pub fn at(t: i64) -> UnixSeconds {
    UnixSeconds(t)
}

pub fn trusted() -> Label {
    Label::trusted_user()
}
pub fn untrusted_mail(in_space: &str) -> Label {
    Label::untrusted(Source::Mail, DataClass::Mail, space(in_space))
}
pub fn lab<T>(value: T, label: Label) -> Labelled<T> {
    Labelled { value, label }
}

pub fn decl(name: &str, effect: Effect, undo: UndoSupport) -> ActionDecl {
    ActionDecl {
        name: action(name),
        label: words("Do it"),
        on: TargetKind::Nothing,
        params: vec![],
        effect,
        classes: BTreeSet::from([DataClass::Mail]),
        undo,
        reach: AgentReach::Offered,
        latency: Latency::Quick,
        result: ResultShape::Nothing,
        keys: KeyHint::None,
        lasting: Lasting::No,
        dry_run: DryRun::None,
        per_call: PerCall::Declared,
    }
}
pub fn text_param(name: &str, sink: ArgSink, need: ParamNeed) -> ParamDecl {
    ParamDecl {
        name: param(name),
        label: words("Field"),
        ty: ParamType::Text {
            max: CharCount(100),
            lines: Lines::One,
        },
        need,
        sink,
        filled: None,
    }
}
