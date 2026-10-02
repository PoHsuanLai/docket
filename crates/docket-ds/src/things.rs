//! A thing as a row shows it ([`ThingMark`], plain strings) and as the router names it
//! ([`EntityRef`], with labels). An app never writes a label: the kind's declared title trust
//! does, so a mail subject is somebody else's words the moment it crosses.

use docket_core::{EntityRef, TitleTrust};
use ds_intents::ThingMark;
use porter_core::{AppName, DataClass};
use prov::{
    Confidentiality, EntityId, EntityKey, EntityKind, Integrity, Label, Labelled, Source, SpaceId,
};
use std::collections::BTreeSet;

/// Why a mark is not a thing.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThingError {
    /// The kind is not a dotted kind.
    #[error("not an entity kind: {0}")]
    Kind(String),
    /// The key is empty, too long or has control characters.
    #[error("not an entity key")]
    Key,
}

/// The row's view of a thing: the words, none of the labels.
pub fn thing_mark(entity: &EntityRef) -> ThingMark {
    ThingMark {
        kind: entity.id.kind.to_string(),
        key: entity.id.key.as_str().to_owned(),
        title: entity.title.value.clone(),
        subtitle: entity.subtitle.value.clone(),
    }
}

fn label_for(titles: &TitleTrust, app: &AppName, space: &SpaceId) -> Label {
    match titles {
        TitleTrust::AppAuthored => Label {
            integrity: Integrity::Trusted,
            confidentiality: Confidentiality::Private(BTreeSet::from([space.clone()])),
            classes: BTreeSet::from([DataClass::AppOwn]),
            sources: BTreeSet::from([Source::App(app.clone())]),
        },
        TitleTrust::ThirdParty(source) => Label {
            integrity: Integrity::Untrusted,
            confidentiality: Confidentiality::Private(BTreeSet::from([space.clone()])),
            classes: BTreeSet::from([DataClass::AppOwn]),
            sources: BTreeSet::from([source.clone()]),
        },
    }
}

/// The router's name for a mark: the app, the kind and key, and titles labelled from the kind's
/// declared `TitleTrust` (third-party text is `Untrusted` from its source).
pub fn entity_ref(
    app: &AppName,
    space: &SpaceId,
    mark: &ThingMark,
    titles: &TitleTrust,
) -> Result<EntityRef, ThingError> {
    let kind = EntityKind::parse(&mark.kind).map_err(|_| ThingError::Kind(mark.kind.clone()))?;
    let key = EntityKey::parse(&mark.key).map_err(|_| ThingError::Key)?;
    let label = label_for(titles, app, space);
    Ok(EntityRef {
        id: EntityId {
            app: app.clone(),
            kind,
            key,
        },
        title: Labelled {
            value: mark.title.clone(),
            label: label.clone(),
        },
        subtitle: Labelled {
            value: mark.subtitle.clone(),
            label,
        },
    })
}
