//! What memory tells a session (`Session.Recall`): recent events, automatic recall, recent
//! episodes, the primer and the person's own profile. The router applies the labels, so untrusted
//! text comes back as a handle and the planner never reads it, and a body that is not trusted
//! never comes back at all.

use crate::handles::HandleValue;
use crate::labels::absorb;
use crate::router::Router;
use crate::seams::{MemoryLink, Seams};
use almanac_core::{
    BodyMode, Episode, FactFilter, FactQuery, KindPattern, MemoryReply, MemoryRequest, RecentEntry,
    RecentQuery, SpaceState, TrustFilter,
};
use docket_core::{
    EpisodeLine, IntentsReply, PrimerText, ProfileLine, RecallAsk, RecallView, RecalledLine,
    RecentLine, Reveal, SkeletonText, WireRefusal,
};
use porter_core::Count;
use prov::{Actor, Integrity, Label, Labelled, SessionId, Source, SpaceId};
use std::collections::BTreeSet;

/// The most lines of a primer.
const PRIMER_LINES: usize = 200;
/// The most facts of a profile.
const PROFILE_FACTS: u32 = 50;

fn refuse(why: WireRefusal) -> IntentsReply {
    IntentsReply::Refused(why)
}

/// A trusted text read into a session still counts when it is private: the session now holds it.
fn absorb_private(record: &mut crate::state::SessionRecord, label: &Label) {
    if !matches!(label.confidentiality, prov::Confidentiality::Public) {
        absorb(record, label);
    }
}

fn source_of(label: &Label) -> Source {
    label.sources.iter().next().cloned().unwrap_or(Source::User)
}

/// The kind of an episode.
fn episode_kind() -> Vec<KindPattern> {
    KindPattern::parse("companion.episode")
        .into_iter()
        .collect()
}

impl<S: Seams> Router<S> {
    /// The Spaces a restart should read: memory's list (the open and paused ones), with the Spaces
    /// this router holds a session or a task in, and the desktop. Memory may refuse the list to
    /// the router (its Spaces read is the shell's today): the router's own Spaces are then the
    /// answer, so a restart still reaches every Space that has been used since the router began.
    async fn recall_spaces(&self) -> IntentsReply {
        let mut spaces: BTreeSet<SpaceId> = BTreeSet::from([SpaceId::desktop()]);
        if let Ok(MemoryReply::Spaces(listed)) =
            self.seams.memory().ask(MemoryRequest::Spaces).await
        {
            spaces.extend(
                listed
                    .into_iter()
                    .filter(|s| matches!(s.state, SpaceState::Open | SpaceState::Paused { .. }))
                    .map(|s| s.id),
            );
        }
        {
            let st = self.locked();
            spaces.extend(st.sessions.values().map(|r| r.space.clone()));
            spaces.extend(st.tasks.all().map(|t| t.space.clone()));
        }
        IntentsReply::Recalled(RecallView::Spaces(spaces.into_iter().collect()))
    }

    /// `.Session.Recall`: memory's answer for this session's Space, every untrusted text a
    /// handle.
    pub(crate) async fn session_recall(&self, id: &SessionId, ask: RecallAsk) -> IntentsReply {
        let Some(space) = self.locked().sessions.get(id).map(|r| r.space.clone()) else {
            return refuse(WireRefusal::NoSuchSession);
        };
        let request = match ask {
            RecallAsk::Spaces => return self.recall_spaces().await,
            RecallAsk::Recent(query) => MemoryRequest::Recent(space, query),
            RecallAsk::Inject(mut query) => {
                query.space = space;
                MemoryRequest::Inject(query)
            }
            RecallAsk::Episodes(query) => MemoryRequest::Recent(
                space,
                RecentQuery {
                    kinds: episode_kind(),
                    trust: TrustFilter::Any,
                    bodies: BodyMode::Json,
                    ..query
                },
            ),
            RecallAsk::Primer => MemoryRequest::Primer(space),
            RecallAsk::Profile => MemoryRequest::Facts(FactQuery {
                space: SpaceId::desktop(),
                topic: None,
                about: None,
                state: FactFilter::Active,
                limit: Count(PROFILE_FACTS),
            }),
        };
        let episodes = matches!(&request, MemoryRequest::Recent(_, q) if q.bodies == BodyMode::Json && q.kinds == episode_kind());
        let Ok(reply) = self.seams.memory().ask(request).await else {
            return refuse(WireRefusal::Malformed);
        };
        if hands_out_untrusted(&reply, episodes)
            && let Err(why) = self.ahead_of_reveal(id, None).await
        {
            return refuse(WireRefusal::Call(why));
        }
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(id) else {
            return refuse(WireRefusal::NoSuchSession);
        };
        let mut reveal = |text: &str, label: &Label| {
            let shown = record.handles.reveal(
                Labelled {
                    value: text.to_owned(),
                    label: label.clone(),
                },
                source_of(label),
            );
            if matches!(shown, Reveal::Plain(_)) && label.integrity == Integrity::Trusted {
                absorb_private(record, label);
            }
            shown
        };
        match reply {
            MemoryReply::Recent(entries) if episodes => {
                let mut mint = |text: &str, label: &Label, from: Source| {
                    record.handles.mint(
                        Labelled {
                            value: HandleValue::Text(text.to_owned()),
                            label: label.clone(),
                        },
                        from,
                    )
                };
                IntentsReply::Recalled(RecallView::Episodes(episode_lines(&entries, &mut mint)))
            }
            MemoryReply::Recent(entries) => IntentsReply::Recalled(RecallView::Recent(
                entries
                    .into_iter()
                    .map(|e| RecentLine {
                        text: e.text.as_ref().map(|t| reveal(t.as_str(), &e.label)),
                        body: e.body.filter(|_| e.label.integrity == Integrity::Trusted),
                        label: e.label,
                        summary: e.summary,
                        effect: e.effect,
                    })
                    .collect(),
            )),
            MemoryReply::Hits(hits) => IntentsReply::Recalled(RecallView::Hits(
                hits.into_iter()
                    .map(|h| RecalledLine {
                        text: reveal(h.text.as_str(), &h.label),
                        doc: h.doc,
                        at: h.at,
                        why: h.why,
                    })
                    .collect(),
            )),
            MemoryReply::Primer(text) => {
                let lines: Vec<&str> = text.lines().take(PRIMER_LINES).collect();
                IntentsReply::Recalled(RecallView::Primer(PrimerText(lines.join("\n"))))
            }
            MemoryReply::Facts(views) => IntentsReply::Recalled(RecallView::Profile(
                views
                    .into_iter()
                    .filter(|v| {
                        matches!(v.fact.by, Actor::User { .. })
                            && v.fact.label.integrity == Integrity::Trusted
                    })
                    .map(|v| ProfileLine(v.fact.text.as_str().to_owned()))
                    .collect(),
            )),
            _ => refuse(WireRefusal::Malformed),
        }
    }
}

/// Whether the recalled `reply` puts untrusted text in a handle: an entry or a hit whose label
/// is untrusted, or an episode (its narrative is a model's, so always).
fn hands_out_untrusted(reply: &MemoryReply, episodes: bool) -> bool {
    match reply {
        MemoryReply::Recent(entries) if episodes => entries.iter().any(|e| e.body.is_some()),
        MemoryReply::Recent(entries) => entries
            .iter()
            .any(|e| e.label.integrity == Integrity::Untrusted),
        MemoryReply::Hits(hits) => hits
            .iter()
            .any(|h| h.label.integrity == Integrity::Untrusted),
        _ => false,
    }
}

/// The episodes of `entries` (newest first) as the planner reads them: one line per episode,
/// the newest event of each (a narrated successor replaces its skeleton), the skeleton as
/// trusted lines and a narrative only by handle. An entry that is not an episode of a trusted
/// skeleton says nothing.
fn episode_lines(
    entries: &[RecentEntry],
    mint: &mut impl FnMut(&str, &Label, Source) -> docket_core::Handle,
) -> Vec<EpisodeLine> {
    let mut seen = BTreeSet::new();
    let mut lines = Vec::new();
    for entry in entries {
        let Some(episode) = entry
            .body
            .as_ref()
            .and_then(|b| serde_json::from_str::<Episode>(b.as_str()).ok())
            .filter(|e| e.skeleton.label.integrity == Integrity::Trusted)
        else {
            continue;
        };
        if !seen.insert(episode.id.clone()) {
            continue;
        }
        let narrative = episode
            .narrative
            .as_ref()
            .map(|n| mint(n.text.as_str(), &n.label, Source::Model(n.by)));
        lines.push(EpisodeLine {
            skeleton: SkeletonText(episode.skeleton.text()),
            id: episode.id,
            agent: episode.agent,
            space: episode.space,
            ended: episode.ended,
            outcome: episode.outcome,
            narrative,
        });
    }
    lines
}
