//! Builders the tests share; labels come from prov's constructors.

use docket_vocab::ParamName;
use porter_core::{AppName, DataClass};
use prov::{
    ActionName, EntityId, EntityKey, EntityKind, Label, Labelled, Source, SpaceId, UnixSeconds,
};

pub fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}
pub fn space(name: &str) -> SpaceId {
    SpaceId::parse(name).expect("space")
}
pub fn action(name: &str) -> ActionName {
    ActionName::parse(name).expect("action")
}
pub fn entity(kind_name: &str, key: &str) -> EntityId {
    EntityId {
        app: app("org.quire.Mail"),
        kind: EntityKind::parse(kind_name).expect("kind"),
        key: EntityKey::parse(key).expect("key"),
    }
}
pub fn at(t: i64) -> UnixSeconds {
    UnixSeconds(t)
}
pub fn untrusted_mail(in_space: &str) -> Label {
    Label::untrusted(Source::Mail, DataClass::Mail, space(in_space))
}
pub fn param(name: &str) -> ParamName {
    ParamName::parse(name).expect("param")
}
pub fn trusted() -> Label {
    Label::trusted_user()
}
pub fn lab<T>(value: T, label: Label) -> Labelled<T> {
    Labelled { value, label }
}
