//! The registry, the shadow index and what is found through it: an app pushes titles so the
//! launcher can search without waking it. The owner is the connection, never a field; the
//! router labels each title from its kind's declared trust, so an app cannot call somebody
//! else's words its own.

use crate::index::IndexAction;
use crate::index::IndexEvent;
use crate::labels::app_label;
use crate::reading::link_refusal;
use crate::router::Router;
use crate::seams::{AppLink, Seams};
use crate::state::RouterState;
use docket_core::{
    CallerId, CallerRole, EntityRef, Hit, IndexBatch, IndexEntry, IndexPolicy, IntentsReply,
    SearchAsk, SearchScope, SuggestAsk, TitleTrust, WireRefusal,
};
use porter_core::AppName;
use prov::{
    Confidentiality, EntityId, EntityKind, Integrity, Label, Labelled, SpaceId, SpaceScope,
};
use std::collections::BTreeSet;

fn refuse(why: WireRefusal) -> IntentsReply {
    IntentsReply::Refused(why)
}

/// The label of a title the app pushed, from the trust its kind declares.
fn title_label(st: &RouterState, app: &AppName, entry: &IndexEntry) -> Label {
    let trust = st
        .registry
        .get(app)
        .and_then(|m| m.manifest().entities.iter().find(|e| e.kind == entry.kind))
        .map(|e| (e.titles.clone(), e.class));
    match trust {
        Some((TitleTrust::AppAuthored, _)) | None => app_label(app),
        Some((TitleTrust::ThirdParty(source), class)) => Label {
            integrity: Integrity::Untrusted,
            confidentiality: Confidentiality::Private(BTreeSet::from([match &entry.space {
                SpaceScope::Only(space) => space.clone(),
                SpaceScope::Any => SpaceId::desktop(),
            }])),
            classes: BTreeSet::from([class]),
            sources: BTreeSet::from([source]),
        },
    }
}

fn matches(entry: &IndexEntry, text: &str) -> bool {
    let needle = text.to_lowercase();
    entry.title.to_lowercase().contains(&needle)
        || entry.subtitle.to_lowercase().contains(&needle)
        || entry
            .keywords
            .iter()
            .any(|k| k.to_lowercase().contains(&needle))
}

impl<S: Seams> Router<S> {
    /// `.Registry.Manifests`.
    pub(crate) fn manifests(&self) -> IntentsReply {
        IntentsReply::Manifests(self.locked().registry.all().cloned().collect())
    }

    /// `.Index.Reset`: the app starts an epoch and what it pushed before is forgotten.
    pub(crate) fn index_reset(&self, caller: &CallerId, epoch: u64) -> IntentsReply {
        let mut st = self.locked();
        let app = caller.app.name.clone();
        if st.registry.get(&app).is_none() {
            return refuse(WireRefusal::NotAllowed);
        }
        st.index_event(&app, IndexEvent::Reset(epoch));
        st.shadow.remove(&app);
        IntentsReply::Done
    }

    /// `.Index.Push`: a batch in the current epoch, for kinds the app declares as indexed.
    pub(crate) fn index_push(&self, caller: &CallerId, batch: IndexBatch) -> IntentsReply {
        let mut st = self.locked();
        let app = caller.app.name.clone();
        let indexed: BTreeSet<EntityKind> = match st.registry.get(&app) {
            Some(m) => m
                .manifest()
                .entities
                .iter()
                .filter(|e| e.index == IndexPolicy::Indexed)
                .map(|e| e.kind.clone())
                .collect(),
            None => return refuse(WireRefusal::NotAllowed),
        };
        match st.index_event(&app, IndexEvent::Push(batch.epoch)) {
            IndexAction::RefuseAskReset => refuse(WireRefusal::Malformed),
            IndexAction::Accept | IndexAction::None => {
                let store = st.shadow.entry(app).or_default();
                for key in &batch.removals {
                    store.retain(|(_, k), _| k != key);
                }
                for entry in batch
                    .upserts
                    .into_iter()
                    .filter(|e| indexed.contains(&e.kind))
                {
                    store.insert((entry.kind.clone(), entry.key.clone()), entry);
                }
                IntentsReply::Done
            }
        }
    }

    /// `.Search.Query`: the shadow index first, then each app that holds kinds it does not
    /// index.
    pub(crate) async fn search_query(&self, role: CallerRole, ask: SearchAsk) -> IntentsReply {
        let (mut hits, live) = {
            let st = self.locked();
            let wanted = |kind: &EntityKind| match &ask.scope {
                SearchScope::Everything => true,
                SearchScope::Kind(k) => k == kind,
            };
            let hits: Vec<Hit> = st
                .shadow
                .iter()
                .flat_map(|(app, entries)| {
                    entries
                        .values()
                        .filter(|e| wanted(&e.kind) && matches(e, &ask.text))
                        .map(|e| {
                            let label = title_label(&st, app, e);
                            Hit {
                                entity: EntityRef {
                                    id: EntityId {
                                        app: app.clone(),
                                        kind: e.kind.clone(),
                                        key: e.key.clone(),
                                    },
                                    title: Labelled {
                                        value: e.title.clone(),
                                        label: label.clone(),
                                    },
                                    subtitle: Labelled {
                                        value: e.subtitle.clone(),
                                        label,
                                    },
                                },
                                why: None,
                            }
                        })
                        .collect::<Vec<_>>()
                })
                .collect();
            let live: Vec<(AppName, BTreeSet<EntityKind>)> = st
                .registry
                .all()
                .map(|m| {
                    let kinds: BTreeSet<EntityKind> = m
                        .manifest()
                        .entities
                        .iter()
                        .filter(|e| e.index == IndexPolicy::NotIndexed && wanted(&e.kind))
                        .map(|e| e.kind.clone())
                        .collect();
                    (m.manifest().app.clone(), kinds)
                })
                .filter(|(_, kinds)| !kinds.is_empty())
                .collect();
            (hits, live)
        };
        for (app, kinds) in live {
            if let Ok(found) = self
                .seams
                .link()
                .search(&app, &ask.text, ask.generation)
                .await
            {
                hits.extend(
                    found
                        .into_iter()
                        .filter(|h| kinds.contains(&h.entity.id.kind)),
                );
            }
        }
        if role == CallerRole::Companion {
            self.show_entities(hits.iter().map(|h| h.entity.id.clone()));
        }
        IntentsReply::Hits(hits)
    }

    /// `.Run.Preview`.
    pub(crate) async fn run_preview(&self, id: &EntityId) -> IntentsReply {
        match self.seams.link().preview(&id.app, id).await {
            Ok(preview) => IntentsReply::Preview(preview),
            Err(fault) => refuse(WireRefusal::Call(link_refusal(fault, &id.app))),
        }
    }

    /// `.Run.Suggest`.
    pub(crate) async fn run_suggest(&self, role: CallerRole, ask: SuggestAsk) -> IntentsReply {
        let app = ask.action.app.clone();
        match self.seams.link().suggest(&app, ask).await {
            Ok(things) => {
                if role == CallerRole::Companion {
                    self.show_entities(things.iter().map(|t| t.id.clone()));
                }
                IntentsReply::Suggestions(things)
            }
            Err(fault) => refuse(WireRefusal::Call(link_refusal(fault, &app))),
        }
    }

    /// The router showed these things to the companion's session, so a name for one is not a
    /// model's invention.
    fn show_entities(&self, ids: impl Iterator<Item = EntityId>) {
        let mut st = self.locked();
        let newest = st
            .sessions
            .iter()
            .filter(|(_, r)| matches!(r.actor, prov::Actor::Companion { .. }))
            .max_by_key(|(id, _)| crate::messaging::age(id))
            .map(|(id, _)| id.clone());
        if let Some(record) = newest.and_then(|id| st.sessions.get_mut(&id)) {
            record.known.extend(ids);
        }
    }
}
