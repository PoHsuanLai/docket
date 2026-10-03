//! Builders the tests share; labels come from prov's constructors.
#![allow(dead_code)]

use docket_core::*;
use porter_core::{AppName, Count, DataClass};
use prov::{
    ActionName, Actor, AgentRole, Effect, EntityId, EntityKey, EntityKind, Label, Labelled,
    SessionId, Source, SpaceId, UnixSeconds,
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
pub fn planner() -> Actor {
    Actor::Companion {
        session: SessionId::parse("s-1").expect("session"),
        role: AgentRole::Planner,
    }
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
    }
}
pub fn manifest(app_name: &str, actions: Vec<ActionDecl>) -> Manifest {
    Manifest {
        vocab: IntentsVocab::CURRENT,
        app: app(app_name),
        entities: vec![],
        actions,
    }
}
pub fn count(n: u32) -> Count {
    Count(n)
}
