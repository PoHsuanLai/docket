//! The registry, the shadow index and what is found through it: an app pushes titles (or ships
//! them in a file) so the launcher can search without waking it, and a search is answered from
//! the index alone: it never asks an app. The owner is the connection, never a field; the
//! router labels each title from its kind's declared trust, so an app cannot call somebody
//! else's words its own.

use crate::index::IndexAction;
use crate::index::IndexEvent;
use crate::labels::app_label;
use crate::rank::{Rank, rank_of};
use crate::reading::link_refusal;
use crate::router::Router;
use crate::seams::{AppLink, Seams};
use crate::state::RouterState;
use docket_core::LinkFault;
use docket_core::{
    CallerId, CallerRole, Hit, IndexBatch, IndexEntry, IntentsReply, SearchAsk, SearchScope,
    SuggestAsk, WireRefusal,
};
use docket_manifest::{IndexPolicy, TitleTrust};
use docket_vocab::EntityRef;
use porter_core::AppName;
use prov::{EntityId, EntityKind, Label, Labelled, SpaceId, SpaceScope, UnixSeconds};
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
        Some((TitleTrust::ThirdParty(source), class)) => Label::untrusted(
            source,
            class,
            match &entry.space {
                SpaceScope::Only(space) => space.clone(),
                SpaceScope::Any => SpaceId::desktop(),
            },
        ),
    }
}

/// The hit for an entry of `app`, its title labelled from the kind's declared trust.
fn hit_of(st: &RouterState, app: &AppName, entry: &IndexEntry) -> Hit {
    let label = title_label(st, app, entry);
    Hit {
        entity: EntityRef {
            id: EntityId {
                app: app.clone(),
                kind: entry.kind.clone(),
                key: entry.key.clone(),
            },
            title: Labelled {
                value: entry.title.clone(),
                label: label.clone(),
            },
            subtitle: Labelled {
                value: entry.subtitle.clone(),
                label,
            },
        },
        why: None,
    }
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

    /// `.Search.Query`: the hits of the index, best match first.
    pub(crate) fn search_query(&self, role: CallerRole, ask: SearchAsk) -> IntentsReply {
        IntentsReply::Hits(self.search_indexed(role, &ask))
    }

    /// The hits of the index: pushed and shipped entries, best match first and newest first
    /// among equals. No app is asked.
    pub fn search_indexed(&self, role: CallerRole, ask: &SearchAsk) -> Vec<Hit> {
        let hits = self.indexed_hits(ask);
        if role == CallerRole::Companion {
            self.show_entities(hits.iter().map(|h| h.entity.id.clone()));
        }
        hits
    }

    fn indexed_hits(&self, ask: &SearchAsk) -> Vec<Hit> {
        let st = self.locked();
        let wanted = |kind: &EntityKind| match &ask.scope {
            SearchScope::Everything => true,
            SearchScope::Kind(k) => k == kind,
        };
        let apps: BTreeSet<&AppName> = st.shadow.keys().chain(st.shipped.keys()).collect();
        let mut found: Vec<(Rank, UnixSeconds, Hit)> = apps
            .into_iter()
            .flat_map(|app| {
                let pushed = st.shadow.get(app);
                // A shipped entry gives way to a pushed one of the same kind and key.
                let own = st.shipped.get(app).into_iter().flat_map(|all| all.iter());
                let own = own.filter(|(id, _)| !pushed.is_some_and(|p| p.contains_key(*id)));
                pushed
                    .into_iter()
                    .flat_map(|p| p.values())
                    .chain(own.map(|(_, e)| e))
                    .filter(|e| wanted(&e.kind))
                    .filter_map(|e| Some((rank_of(e, &ask.text)?, e)))
                    .map(|(rank, e)| (rank, e.updated, hit_of(&st, app, e)))
                    .collect::<Vec<_>>()
            })
            .collect();
        found.sort_by_key(|(rank, updated, _)| (*rank, std::cmp::Reverse(*updated)));
        found.into_iter().map(|(_, _, hit)| hit).collect()
    }

    /// `.Run.Preview`.
    pub(crate) async fn run_preview(&self, id: &EntityId) -> IntentsReply {
        if !self.seams.link().running(&id.app).await {
            return refuse(WireRefusal::Call(link_refusal(
                LinkFault::Unavailable,
                &id.app,
            )));
        }
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
            .max_by_key(|(id, _)| crate::session::age(id))
            .map(|(id, _)| id.clone());
        if let Some(record) = newest.and_then(|id| st.sessions.get_mut(&id))
            && let Err(limit) = record.know(ids)
        {
            tracing::warn!(task = %record.task, %limit, "things shown were not noted");
        }
    }
}
