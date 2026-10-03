//! Literal labels for fixtures: the fakes write the few labels they need out in full, so a
//! fixture shows exactly what an app reports.

use docket_core::{EntityRef, Follow, LabelText, Outcome, Preview, Undoable};
use porter_core::{AppName, DataClass};
use prov::{
    Confidentiality, EntityId, EntityKey, EntityKind, Integrity, Label, Labelled, Source, SpaceId,
};
use std::collections::BTreeSet;

/// The label of text an app wrote itself.
pub fn app_label(app: &AppName) -> Label {
    Label {
        integrity: Integrity::Trusted,
        confidentiality: Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::App(app.clone())]),
    }
}

/// The label of somebody else's words from `source`, private to `space`.
pub fn third_party(source: Source, class: DataClass, space: SpaceId) -> Label {
    Label::untrusted(source, class, space)
}

/// An entity id for `app`, or `None` if the kind or key is malformed.
pub fn entity(app: &AppName, kind: &str, key: &str) -> Option<EntityId> {
    Some(EntityId {
        app: app.clone(),
        kind: EntityKind::parse(kind).ok()?,
        key: EntityKey::parse(key).ok()?,
    })
}

/// A thing with its words labelled.
pub fn entity_ref(id: EntityId, title: Labelled<String>, subtitle: Labelled<String>) -> EntityRef {
    EntityRef {
        id,
        title,
        subtitle,
    }
}

/// An outcome with an optional spoken line (dropped if it is not valid UI words).
pub fn outcome(said: Option<String>, undo: Undoable, show: Preview) -> Outcome {
    Outcome {
        value: None,
        said: said.and_then(|s| LabelText::parse(&s).ok()),
        show,
        undo,
        follow: Follow::Nothing,
    }
}
