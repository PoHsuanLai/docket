//! A session's durable log over almanac: `docket-session`'s `SessionLog` through `RecordDurable`
//! (an append answers only when the event is on disk) and `Entries` (one session's events in
//! append order, found by the session thing they name as their `Subject`).
//!
//! Each entry is an `Area { Companion }` event of kind `companion.session.<slug>`; almanac keeps
//! those out of recall. The position (`Seq`) is docket's own count carried in the stored body,
//! because almanac's sequence is the Space's and interleaves its own audit events. The log lives
//! in one Space, named when it is built.

use crate::cursor_cache::CursorCache;
use almanac_client::{ClientError, Memory, Transport};
use almanac_core::{
    AreaPayload, AreaTag, BodyMode, Cause, Cursor, EntriesQuery, EventBody, JsonText, KindPattern,
    KindTag, Record, Refusal, ThingRole, ThingView, UserText,
};
use docket_router::Clock;
use docket_session::{
    Appended, LogFault, LogPage, PageSize, Seq, SessionEntry, SessionLog, decode, encode,
};
use porter_core::{AppName, Count};
use prov::{
    Actor, Confidentiality, EntityId, EntityKey, EntityKind, Integrity, Label, SessionId, Source,
    SpaceId, SystemPart,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

/// The app whose things the sessions are.
const SESSION_APP: &str = "org.quire.Companion";
/// The kind of the thing a session names itself as.
const SESSION_KIND: &str = "companion.session";
/// The kind of the entry every session starts with.
const OPENED_KIND: &str = "companion.session.opened";
/// The kinds a session's entries have.
const ENTRY_KINDS: &str = "companion.session.*";

/// Rows read at a time when the tail of a log is looked for.
const TAIL_PAGE: u32 = 200;

/// A session's log in one almanac Space, over any `almanac_client::Transport`.
#[derive(Debug)]
pub struct AlmanacSessionLog<T: Transport, K: Clock> {
    memory: Memory<T>,
    space: SpaceId,
    clock: K,
    /// The next position of each session this writer has seen the end of.
    next: Mutex<BTreeMap<SessionId, Seq>>,
    /// Where almanac's cursor stood after the rows a page ended on.
    cursors: CursorCache,
}

/// The end of a log as read.
struct Tail {
    next: Seq,
    last: Option<String>,
}

impl<T: Transport, K: Clock> AlmanacSessionLog<T, K> {
    /// The log of every session, kept in `space`, over `transport`, stamped by `clock`.
    pub fn over(transport: T, space: SpaceId, clock: K) -> Self {
        Self {
            memory: Memory::over(transport),
            space,
            clock,
            next: Mutex::new(BTreeMap::new()),
            cursors: CursorCache::default(),
        }
    }

    fn thing(session: &SessionId) -> Result<EntityId, LogFault> {
        Ok(EntityId {
            app: AppName::parse(SESSION_APP).map_err(|_| LogFault::Encode)?,
            kind: EntityKind::parse(SESSION_KIND).map_err(|_| LogFault::Encode)?,
            key: EntityKey::parse(session.as_str()).map_err(|_| LogFault::Encode)?,
        })
    }

    /// What the router says about its own record: trusted, private to the Space.
    fn label(&self) -> Label {
        Label {
            integrity: Integrity::Trusted,
            confidentiality: Confidentiality::Private(BTreeSet::from([self.space.clone()])),
            classes: BTreeSet::new(),
            sources: BTreeSet::from([Source::App(
                AppName::parse("org.quire.Intents1")
                    .unwrap_or_else(|_| unreachable!("`org.quire.Intents1` is a valid app name")),
            )]),
        }
    }

    fn record(&self, session: &SessionId, kind: &str, json: &str) -> Result<Record, LogFault> {
        let body = EventBody::Area(AreaPayload {
            area: AreaTag::Companion,
            kind: KindTag::parse(kind).map_err(|_| LogFault::Encode)?,
            json: JsonText::parse(json).map_err(|_| LogFault::Encode)?,
            things: vec![(
                ThingView {
                    thing: Self::thing(session)?,
                    title: UserText::new(""),
                    subtitle: UserText::new(""),
                },
                ThingRole::Subject,
            )],
        });
        Ok(Record {
            space: self.space.clone(),
            occurred: self.clock.now(),
            actor: Actor::System {
                part: SystemPart::Router,
            },
            effect: prov::Effect::Read,
            label: self.label(),
            body,
            cause: Cause::None,
        })
    }

    fn query(
        &self,
        session: &SessionId,
        after: Option<Cursor>,
        limit: u32,
    ) -> Result<EntriesQuery, LogFault> {
        Ok(EntriesQuery {
            kinds: vec![KindPattern::parse(ENTRY_KINDS).map_err(|_| LogFault::Encode)?],
            about: Some(Self::thing(session)?),
            after,
            limit: Count(limit),
            bodies: BodyMode::Json,
        })
    }

    /// Reads to the end of `session`'s log: the next position and the last body.
    async fn tail(&self, session: &SessionId) -> Result<Tail, LogFault> {
        let mut after = None;
        let mut count = 0u64;
        let mut last = None;
        loop {
            let query = self.query(session, after, TAIL_PAGE)?;
            let page = self
                .memory
                .entries(self.space.clone(), query)
                .await
                .map_err(fault_of)?;
            for entry in &page.entries {
                count += 1;
                last = entry.body.as_ref().map(|b| b.as_str().to_owned());
            }
            match page.next {
                Some(cursor) => after = Some(cursor),
                None => break,
            }
        }
        // The position of the last row is the count when the log is whole; a gap shows at
        // resume, from the stored `seq`s, not here.
        let next = match last.as_deref().and_then(stored_seq) {
            Some(seq) => seq.next(),
            None => Seq(count),
        };
        Ok(Tail { next, last })
    }

    fn remember(&self, session: &SessionId, next: Seq) {
        if let Ok(mut all) = self.next.lock() {
            all.insert(session.clone(), next);
        }
    }

    fn forget(&self, session: &SessionId) {
        if let Ok(mut all) = self.next.lock() {
            all.remove(session);
        }
        self.cursors.forget(session);
    }

    fn cached(&self, session: &SessionId) -> Option<Seq> {
        self.next.lock().ok()?.get(session).copied()
    }
}

/// The `seq` a stored body carries.
fn stored_seq(json: &str) -> Option<Seq> {
    serde_json::from_str::<serde_json::Value>(json)
        .ok()?
        .get("seq")
        .and_then(serde_json::Value::as_u64)
        .map(Seq)
}

/// What a failed call to almanac means to the log: a store that will not remember is `Refused`;
/// one that cannot answer (a lock, no daemon, a failing disk) is `Unavailable`. Either refuses
/// the reveal an entry guards.
fn fault_of(error: ClientError) -> LogFault {
    match error {
        ClientError::Refused(Refusal::SpaceLocked | Refusal::Unavailable | Refusal::Busy) => {
            LogFault::Unavailable
        }
        ClientError::Refused(_) => LogFault::Refused,
        // An unknown failure counts as unavailable, so the reveal it guards is refused.
        ClientError::Transport(_) | ClientError::Unexpected => LogFault::Unavailable,
        _ => LogFault::Unavailable,
    }
}

impl<T: Transport, K: Clock> SessionLog for AlmanacSessionLog<T, K> {
    async fn append(
        &self,
        session: &SessionId,
        seq: Seq,
        entry: &SessionEntry,
    ) -> Result<Appended, LogFault> {
        let encoded = encode(seq, entry).map_err(|_| LogFault::Encode)?;
        let (next, last) = match self.cached(session) {
            Some(next) => (next, None),
            None => {
                let tail = self.tail(session).await?;
                self.remember(session, tail.next);
                (tail.next, tail.last)
            }
        };
        if seq != next {
            // An earlier try that was not acknowledged may have landed: the same entry at the
            // position just written is that try, acknowledged now.
            let landed = seq.next() == next && last.as_deref() == Some(encoded.json.as_str());
            return match landed {
                true => Ok(Appended { seq }),
                false => Err(LogFault::OutOfOrder { expected: next }),
            };
        }
        let record = self.record(session, &encoded.kind, &encoded.json)?;
        match self.memory.record_durable(record).await {
            Ok(_) => {
                self.remember(session, seq.next());
                Ok(Appended { seq })
            }
            Err(error) => {
                // Unknown whether it landed: the next append reads the end of the log again.
                self.forget(session);
                Err(fault_of(error))
            }
        }
    }

    async fn page(
        &self,
        session: &SessionId,
        from: Option<Seq>,
        size: PageSize,
    ) -> Result<LogPage, LogFault> {
        let want = size.0.0.max(1);
        let start = from.map_or(0, |s| s.0);
        let known = self.cursors.at(session, Seq(start));
        // With the cursor of `start` the read resumes there; without one it counts from the
        // first row.
        let (mut after, mut place) = match (start, known) {
            (0, _) => (None, 0),
            (_, Some(cursor)) => (Some(cursor), start),
            (_, None) => (None, 0),
        };
        let mut rows = Vec::new();
        let mut more = false;
        loop {
            let query = self.query(session, after, want)?;
            let page = self
                .memory
                .entries(self.space.clone(), query)
                .await
                .map_err(fault_of)?;
            for entry in &page.entries {
                if place >= start && rows.len() < want as usize {
                    let slug = entry.summary.kind.as_str().rsplit('.').next().unwrap_or("");
                    let body = entry.body.as_ref().map_or("", JsonText::as_str);
                    rows.push(decode(slug, body, Seq(place)));
                } else if place >= start {
                    more = true;
                }
                place += 1;
            }
            if let Some(cursor) = page.next {
                self.cursors.stop(session, Seq(place), cursor);
            }
            after = page.next;
            if after.is_none() || rows.len() >= want as usize {
                more = more || after.is_some();
                break;
            }
        }
        let next = more.then_some(Seq(start + rows.len() as u64));
        Ok(LogPage { rows, next })
    }

    async fn sessions(&self) -> Result<Vec<SessionId>, LogFault> {
        let mut found = Vec::new();
        let mut seen = BTreeSet::new();
        let mut after = None;
        loop {
            let query = EntriesQuery {
                kinds: vec![KindPattern::parse(OPENED_KIND).map_err(|_| LogFault::Encode)?],
                about: None,
                after,
                limit: Count(TAIL_PAGE),
                bodies: BodyMode::Without,
            };
            let page = self
                .memory
                .entries(self.space.clone(), query)
                .await
                .map_err(fault_of)?;
            for entry in &page.entries {
                let named = entry
                    .summary
                    .things
                    .iter()
                    .find(|t| t.thing.kind.as_str() == SESSION_KIND)
                    .and_then(|t| SessionId::parse(t.thing.key.as_str()).ok());
                found.extend(named.filter(|id| seen.insert(id.clone())));
            }
            match page.next {
                Some(cursor) => after = Some(cursor),
                None => return Ok(found),
            }
        }
    }
}
