//! The helpers' records: written through the router (`Session.Note`) as `Area { Helpers }` events,
//! and read back after a restart through `Session.Recall` in each Space, the way the companion's
//! own session records are. A record that cannot be written costs the restart that record; one
//! that cannot be read is left out and said in the journal.

use crate::best_effort::best_effort;
use crate::recover::{RecentSource, RouterRecent, owner_form};
use crate::runtime::Companion;
use crate::seams::{Now, Surface};
use almanac_core::{BodyMode, JsonText, KindPattern, RecentEntry, RecentQuery, TrustFilter};
use docket_client::Transport as IntentsTransport;
use docket_core::helper::{HelperRecord, HelperRecordKind};
use docket_core::{HelperNote, NoteAsk, SessionOpen};
use porter_client::Transport as InferTransport;
use porter_core::Count;
use prov::{AgentRef, SpaceId, UnixSeconds};

/// The most records one Space's read returns.
const MOST_RECORDS: u32 = 5_000;

/// What a restart asks memory for: every kind of helper record, from the beginning of time. A
/// helper outlives the 30 days the companion's session records are read back for.
pub fn helper_query() -> RecentQuery {
    RecentQuery {
        since: UnixSeconds(0),
        kinds: HelperRecordKind::ALL
            .iter()
            .filter_map(|kind| KindPattern::parse(kind.slug()).ok())
            .collect(),
        trust: TrustFilter::Any,
        limit: Count(MOST_RECORDS),
        bodies: BodyMode::Json,
    }
}

/// What a restart asks memory for to bring the routines back: their records, from the beginning of
/// time.
pub fn routine_query() -> RecentQuery {
    RecentQuery {
        kinds: vec![KindPattern::parse(HelperRecordKind::Routine.slug()).expect("a kind pattern")],
        ..helper_query()
    }
}

/// The helper records in `entries` (newest first, as `Recent` answers), oldest first. An entry
/// that is not one of ours, has no body or does not parse is left out.
pub fn helper_records_of(entries: &[RecentEntry]) -> Vec<HelperRecord> {
    entries
        .iter()
        .rev()
        .filter_map(|entry| {
            let kind = HelperRecordKind::of_slug(entry.summary.kind.as_str())?;
            let body = entry.body.as_ref()?;
            let text = owner_form(body.as_str());
            match HelperRecord::from_json(kind, text.as_ref()) {
                Ok(record) => Some(record),
                Err(cause) => {
                    tracing::warn!(%cause, "a helper record is not readable; left out");
                    None
                }
            }
        })
        .collect()
}

impl<P: InferTransport, I: IntentsTransport, K: Now, S: Surface> Companion<P, I, K, S> {
    /// Writes `records`, all of the helpers of `space`, through a session opened in that Space for
    /// the purpose and closed again. A router that does not answer loses them and says so.
    pub async fn write_helper_records(&self, space: &SpaceId, records: &[HelperRecord]) {
        if records.is_empty() {
            return;
        }
        let opened = self
            .intents
            .session_open(SessionOpen {
                space: space.clone(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
                helper: None,
                acts_for: None,
            })
            .await;
        let Ok(opened) = opened else {
            tracing::warn!(?space, "no session to write the helpers' records through");
            return;
        };
        for record in records {
            let Ok(text) = record.json() else {
                continue;
            };
            let Ok(json) = JsonText::parse(&text) else {
                continue;
            };
            let note = NoteAsk::Helper(HelperNote {
                kind: record.kind(),
                json,
            });
            let written = self
                .intents
                .session_note(opened.session.clone(), note)
                .await;
            best_effort("session.note.helper", &opened.session, written);
        }
        let who = opened.session.clone();
        let closed = self.intents.session_close(opened.session).await;
        best_effort("session.close", &who, closed);
    }

    /// Every helper record memory holds for the Spaces `named` and the ones the router knows,
    /// oldest first within a Space. A Space that cannot be read adds nothing.
    pub async fn read_helper_records(&self, named: &[SpaceId]) -> Vec<HelperRecord> {
        self.read_records(named, helper_query).await
    }

    /// The routines' records, read the same way.
    pub async fn read_routine_records(&self, named: &[SpaceId]) -> Vec<HelperRecord> {
        self.read_records(named, routine_query).await
    }

    async fn read_records(
        &self,
        named: &[SpaceId],
        query: fn() -> RecentQuery,
    ) -> Vec<HelperRecord> {
        let mut records = Vec::new();
        for space in &self.spaces_to_read(named).await {
            let reading = self
                .intents
                .session_open(SessionOpen {
                    space: space.clone(),
                    agent: AgentRef::Companion,
                    parent: None,
                    cwd: None,
                    started_from: None,
                    external: None,
                    helper: None,
                    acts_for: None,
                })
                .await;
            let Ok(reading) = reading else {
                tracing::warn!(?space, "no session to read the helpers' records through");
                continue;
            };
            let found = RouterRecent::new(&self.intents, reading.session.clone())
                .recent(query())
                .await;
            let who = reading.session.clone();
            let closed = self.intents.session_close(reading.session).await;
            best_effort("session.close", &who, closed);
            match found {
                Ok(entries) => records.extend(helper_records_of(&entries)),
                Err(cause) => {
                    tracing::warn!(?space, %cause, "the helpers' records could not be read");
                }
            }
        }
        records
    }
}
