//! The context seam of the acceptance run's mail window.

use super::Focus;
use crate::provider::threads;
use docket_core::{
    ContextScope, ContextSnapshot, EntityRef, Here, Selection, SummonAnswer, SummonOrigin,
    SummonSerial, TextTarget, Visible, WindowPrivacy,
};
use docket_provider::{ContextSource, SummonTarget};
use porter_core::{AppName, DataClass};
use prov::{
    Confidentiality, EntityId, EntityKey, EntityKind, Integrity, Label, Labelled, Source, SpaceId,
};
use std::collections::BTreeSet;

/// The context seam: the acceptance run never summons from inside the app, so the window it
/// reports is a quiet one, looking at `focus`.
#[derive(Debug, Clone)]
pub struct QuietWindow {
    /// The app.
    pub app: AppName,
    /// What is open in its window.
    pub focus: Focus,
    /// The window's name.
    pub window: &'static str,
}

impl QuietWindow {
    fn here(&self) -> Here {
        let Focus::Thread(key) = self.focus else {
            return Here::Nowhere;
        };
        let label = Label::untrusted(
            Source::Mail,
            DataClass::Mail,
            SpaceId::parse("work").unwrap_or_else(|_| SpaceId::desktop()),
        );
        let found = threads().into_iter().find(|t| t.key == key);
        let id = EntityKind::parse("mail.thread")
            .ok()
            .zip(EntityKey::parse(key).ok())
            .map(|(kind, key)| EntityId {
                app: self.app.clone(),
                kind,
                key,
            });
        match (found, id) {
            (Some(t), Some(id)) => Here::Entity(EntityRef {
                id,
                title: Labelled {
                    value: t.subject.to_owned(),
                    label: label.clone(),
                },
                subtitle: Labelled {
                    value: t.from.to_owned(),
                    label,
                },
            }),
            _ => Here::Nowhere,
        }
    }
}

impl ContextSource for QuietWindow {
    fn snapshot(&self, _scope: ContextScope) -> ContextSnapshot {
        ContextSnapshot {
            app: self.app.clone(),
            window: Labelled {
                value: self.window.to_owned(),
                label: Label {
                    integrity: Integrity::Trusted,
                    confidentiality: Confidentiality::Public,
                    classes: BTreeSet::new(),
                    sources: BTreeSet::from([Source::App(self.app.clone())]),
                },
            },
            here: self.here(),
            selection: Selection::Nothing,
            visible: Visible {
                kind: None,
                items: vec![],
                total: porter_core::Count(0),
            },
            text_target: TextTarget::None,
            privacy: WindowPrivacy::Normal,
        }
    }
}

impl SummonTarget for QuietWindow {
    fn summon(&self, _serial: SummonSerial, _origin: SummonOrigin) -> SummonAnswer {
        SummonAnswer::Declined
    }
}
