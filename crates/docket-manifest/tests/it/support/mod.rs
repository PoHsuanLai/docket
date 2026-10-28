//! Builders the tests share; labels come from prov's constructors.

use docket_manifest::{
    ActionDecl, AgentReach, ArgSink, DryRun, EntityDecl, IndexPolicy, KeyHint, Lasting, Latency,
    Manifest, ParamDecl, ParamNeed, ResultShape, TargetKind, TitleTrust, UndoSupport, Visibility,
};
use docket_vocab::{
    CharCount, IconName, IntentsVocab, LabelText, Lines, ParamName, ParamType, PerCall,
};
use porter_core::{AppName, DataClass};
use prov::{ActionName, Effect, EntityId, EntityKey, EntityKind, Label};
use std::collections::BTreeSet;

pub fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
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
pub fn trusted() -> Label {
    Label::trusted_user()
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
pub fn manifest(app_name: &str, actions: Vec<ActionDecl>) -> Manifest {
    Manifest {
        vocab: IntentsVocab::CURRENT,
        app: app(app_name),
        entities: vec![],
        actions,
        visibility: Visibility::Everyone,
    }
}
/// A mail thread kind with the given index policy and nothing else declared.
pub fn thread_kind(index: IndexPolicy) -> EntityDecl {
    EntityDecl {
        kind: kind("mail.thread"),
        label: words("Thread"),
        plural: words("Threads"),
        icon: IconName::parse("mail").expect("icon"),
        class: DataClass::Mail,
        index,
        titles: TitleTrust::AppAuthored,
        props: vec![],
        relations: vec![],
    }
}
