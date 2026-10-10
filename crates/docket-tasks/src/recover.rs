//! Restart. companiond keeps no state of its own: the roster and the front task are rebuilt from
//! the sessions the router stores (`stored_roster`, read through `Session.Stored`) and from the
//! records the eventlog holds (the router's messages, the episodes and the computer-use runs),
//! read through memory's `Recent` with `BodyMode::Json`. Each entry's body is the owner's serde
//! form, so an entry that does not parse is a fault, never a guess.
//!
//! The legacy session notes (`companion.session.*`) are still read from `Recent`, but used only
//! when the router stores no sessions at all: a host whose router keeps no session log (the in-app
//! agent) has nothing else to rebuild its sessions from, and a log that has just begun may have
//! older notes.

use agent_loop::{Rebuilt, ReplayEvent, ReplayWhat, rebuild};
use almanac_core::{BodyMode, Episode, KindPattern, RecentEntry, RecentQuery, TrustFilter};
use companion_wire::{SESSION_KIND_PREFIX, SessionRecord};
use docket_core::RosterState;
use porter_core::Count;
use prov::{Message, RunId, SpaceId, UnixSeconds};
use std::collections::BTreeMap;
use std::future::Future;

/// The kind of a message the router delivered.
const MESSAGE_KIND: &str = "companion.message";
/// The kind of an episode.
const EPISODE_KIND: &str = "companion.episode";
/// The kinds of a computer-use run the rebuild reads: where it began, what it asked, who has the
/// window, how it ended. Its steps are not read: they say nothing the roster shows. cuad writes
/// them as `Area { Cua }` payloads in `cua-bus`'s `CuaRecord` form; docket names no cua type and
/// reads only the words below.
const RUN_KINDS: [&str; 6] = [
    "cua.run.started",
    "cua.asked",
    "cua.confirmed",
    "cua.taken_over",
    "cua.handed_back",
    "cua.run.finished",
];
/// The most events one restart reads.
const RESTART_LIMIT: u32 = 5000;

/// Why the stored records could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ReplayFault {
    /// The router or memoryd is not there.
    #[error("the record is not available")]
    Unavailable,
    /// A record is not in the form its owner writes.
    #[error("a stored record is malformed")]
    Malformed,
}

/// Where `Recent` is answered: memoryd through the router's `Session.Recall`, or a fake.
pub trait RecentSource: Send + Sync {
    /// The entries `query` asks for, newest first.
    fn recent(
        &self,
        query: RecentQuery,
    ) -> impl Future<Output = Result<Vec<RecentEntry>, ReplayFault>> + Send;
}

/// `Recent` through the router: `Session.Recall` in a session of the Space to read. The router
/// hands over a body only for an entry whose label is trusted (a message that carries what a worker
/// read, or a narrated episode, comes without one and says nothing here), and the Space is the
/// session's: a restart reads each Space it knows through a session of its own.
#[derive(Debug)]
pub struct RouterRecent<'a, I: docket_client::Transport> {
    intents: &'a docket_client::Intents<I>,
    session: prov::SessionId,
}

impl<'a, I: docket_client::Transport> RouterRecent<'a, I> {
    /// Reads the Space of `session` through `intents`.
    pub fn new(intents: &'a docket_client::Intents<I>, session: prov::SessionId) -> Self {
        Self { intents, session }
    }
}

impl<I: docket_client::Transport> RecentSource for RouterRecent<'_, I> {
    async fn recent(&self, query: RecentQuery) -> Result<Vec<RecentEntry>, ReplayFault> {
        let view = self
            .intents
            .session_recall(self.session.clone(), docket_core::RecallAsk::Recent(query))
            .await
            .map_err(|_| ReplayFault::Unavailable)?;
        let docket_core::RecallView::Recent(lines) = view else {
            return Err(ReplayFault::Unavailable);
        };
        Ok(lines
            .into_iter()
            .map(|line| RecentEntry {
                text: match line.text {
                    Some(docket_core::Reveal::Plain(text)) => {
                        Some(almanac_core::UserText::new(text))
                    }
                    Some(docket_core::Reveal::Handle(_)) | None => None,
                },
                summary: line.summary,
                effect: line.effect,
                label: line.label,
                body: line.body,
            })
            .collect())
    }
}

/// The query a restart sends: every kind the rebuild reads, with bodies, since the retention
/// window opened.
pub fn restart_query(since: UnixSeconds) -> RecentQuery {
    RecentQuery {
        since,
        kinds: [
            format!("{SESSION_KIND_PREFIX}.*"),
            MESSAGE_KIND.to_owned(),
            EPISODE_KIND.to_owned(),
        ]
        .iter()
        .map(String::as_str)
        .chain(RUN_KINDS)
        .filter_map(|k| KindPattern::parse(k).ok())
        .collect(),
        trust: TrustFilter::Any,
        limit: Count(RESTART_LIMIT),
        bodies: BodyMode::Json,
    }
}

/// The owner's form of a stored body. Memory's `Recent` is documented to answer an `Area` payload
/// as its owner wrote it, but the service answers the whole body, `{"kind":"area","v":{..,
/// "json":"<the owner's form>"}}`; both are read, so a record is rebuilt from either.
fn owner_form(text: &str) -> std::borrow::Cow<'_, str> {
    let wrapped = serde_json::from_str::<serde_json::Value>(text)
        .ok()
        .filter(|body| body["kind"] == "area")
        .and_then(|body| body["v"]["json"].as_str().map(str::to_owned));
    match wrapped {
        Some(inner) => std::borrow::Cow::Owned(inner),
        None => std::borrow::Cow::Borrowed(text),
    }
}

fn what_of(entry: &RecentEntry) -> Result<Option<ReplayWhat>, ReplayFault> {
    let kind = entry.summary.kind.as_str();
    let wanted = kind == MESSAGE_KIND
        || kind == EPISODE_KIND
        || kind
            .strip_prefix(SESSION_KIND_PREFIX)
            .is_some_and(|rest| rest.starts_with('.'));
    if !wanted {
        return Ok(None);
    }
    // Header-only or erased events have no body: nothing to rebuild from.
    let Some(body) = &entry.body else {
        return Ok(None);
    };
    let text = owner_form(body.as_str());
    let text = text.as_ref();
    let malformed = |_| ReplayFault::Malformed;
    let what = match kind {
        MESSAGE_KIND => ReplayWhat::Message(Box::new(
            serde_json::from_str::<Message>(text).map_err(malformed)?,
        )),
        EPISODE_KIND => ReplayWhat::Episode(Box::new(
            serde_json::from_str::<Episode>(text).map_err(malformed)?,
        )),
        _ => ReplayWhat::Session(Box::new(
            serde_json::from_str::<SessionRecord>(text).map_err(malformed)?,
        )),
    };
    Ok(Some(what))
}

/// Where a run stands after one of its records, from the record's kind and body.
fn run_state(kind: &str, record: &serde_json::Value) -> Option<RosterState> {
    Some(match kind {
        "cua.run.started" | "cua.confirmed" | "cua.handed_back" => RosterState::Working,
        "cua.asked" => RosterState::NeedsYou,
        "cua.taken_over" => RosterState::Paused,
        "cua.run.finished" => match record["v"]["outcome"]["kind"].as_str()? {
            "done" => RosterState::Done,
            "cancelled" => RosterState::Cancelled,
            "failed" | "blocked" | "budget_out" => RosterState::Failed,
            _ => return None,
        },
        _ => return None,
    })
}

/// A record of a computer-use run as the rebuild's `Run` event. Only `cua.run.started` names the
/// Space, so the Space of each run seen so far is kept in `spaces` (oldest record first); a run
/// whose start is older than the window has no Space and says nothing.
fn run_of(
    entry: &RecentEntry,
    spaces: &mut BTreeMap<RunId, SpaceId>,
) -> Result<Option<ReplayWhat>, ReplayFault> {
    let kind = entry.summary.kind.as_str();
    if !RUN_KINDS.contains(&kind) {
        return Ok(None);
    }
    let Some(body) = &entry.body else {
        return Ok(None);
    };
    let malformed = |_| ReplayFault::Malformed;
    let record: serde_json::Value =
        serde_json::from_str(&owner_form(body.as_str())).map_err(malformed)?;
    let run = record["v"]["run"]
        .as_str()
        .and_then(|r| RunId::parse(r).ok())
        .ok_or(ReplayFault::Malformed)?;
    if kind == "cua.run.started" {
        let space = record["v"]["space"]
            .as_str()
            .and_then(|s| SpaceId::parse(s).ok())
            .ok_or(ReplayFault::Malformed)?;
        spaces.insert(run.clone(), space);
    }
    let state = run_state(kind, &record).ok_or(ReplayFault::Malformed)?;
    Ok(spaces.get(&run).map(|space| ReplayWhat::Run {
        run,
        space: space.clone(),
        state,
    }))
}

/// The rebuild's events from `Recent`'s entries (newest first in, oldest first out). Entries of
/// other kinds are skipped; one of ours that does not parse is `Malformed`.
pub fn replay_of(entries: &[RecentEntry]) -> Result<Vec<ReplayEvent>, ReplayFault> {
    let mut events = Vec::new();
    let mut spaces = BTreeMap::new();
    for entry in entries.iter().rev() {
        let what = match run_of(entry, &mut spaces)? {
            Some(run) => Some(run),
            None => what_of(entry)?,
        };
        if let Some(what) = what {
            events.push(ReplayEvent {
                at: entry.summary.occurred,
                what,
            });
        }
    }
    Ok(events)
}

/// The events `source` holds since `since`: session notes, messages, episodes and runs, oldest
/// first.
pub async fn recent_events<S: RecentSource>(
    source: &S,
    since: UnixSeconds,
) -> Result<Vec<ReplayEvent>, ReplayFault> {
    replay_of(&source.recent(restart_query(since)).await?)
}

/// Rebuilds the roster and the front task from what `source` holds since `since`, session notes
/// included (the sessions the router stores come from `stored_events`, and `rebuild_from` joins
/// the two).
pub async fn recover<S: RecentSource>(
    source: &S,
    since: UnixSeconds,
) -> Result<Rebuilt, ReplayFault> {
    Ok(rebuild(&recent_events(source, since).await?))
}

/// Rebuilds from the sessions the router stores (`stored`: `None` when it stores none) and the
/// records memory holds, in time order. The session notes in memory count only when the router
/// stores no sessions; otherwise the stored sessions are the truth, and an editor's session
/// (left out of them) is not brought back by an older note. A session's events come first among
/// equals: its own records are older than the episode that ends it.
pub fn rebuild_from(stored: Option<Vec<ReplayEvent>>, recent: Vec<ReplayEvent>) -> Rebuilt {
    let mut events = match stored {
        Some(stored) => {
            let others = recent
                .into_iter()
                .filter(|event| !matches!(event.what, ReplayWhat::Session(_)));
            stored.into_iter().chain(others).collect()
        }
        None => recent,
    };
    events.sort_by_key(|event| event.at);
    rebuild(&events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use companion_wire::SessionRecord;
    use prov::{AgentRef, TaskId};

    fn opened(task: &str, at: i64) -> ReplayEvent {
        ReplayEvent {
            at: UnixSeconds(at),
            what: ReplayWhat::Session(Box::new(SessionRecord::Opened {
                task: TaskId::parse(task).expect("task"),
                space: SpaceId::parse("work").expect("space"),
                agent: AgentRef::Companion,
                parent: None,
            })),
        }
    }

    #[test]
    fn session_notes_in_memory_count_only_when_the_router_stores_no_sessions() {
        // No stored sessions (a router with no session log): the notes are what there is.
        let from_notes = rebuild_from(None, vec![opened("t-1", 10)]);
        assert_eq!(from_notes.tasks.len(), 1);
        // Some stored sessions, even ones that add no events (an editor's): the notes are older
        // facts the stored view has superseded, and an editor's session is not brought back by one.
        let superseded = rebuild_from(Some(vec![]), vec![opened("t-1", 10)]);
        assert_eq!(superseded.tasks.len(), 0);
        assert_eq!(superseded.front, None);
        // Stored events win and the rest of memory is kept.
        let both = rebuild_from(Some(vec![opened("t-2", 5)]), vec![opened("t-1", 10)]);
        assert_eq!(both.tasks.len(), 1);
        assert_eq!(both.front, Some(TaskId::parse("t-2").expect("task")));
    }

    const BARE: &str = r#"{"kind":"closed"}"#;

    #[test]
    fn a_body_is_read_the_same_bare_or_wrapped_in_the_area_envelope() {
        let wrapped = serde_json::json!({
            "kind": "area",
            "v": { "area": "companion", "kind": "companion.session.closed", "json": BARE, "things": [] }
        })
        .to_string();
        assert_eq!(owner_form(BARE), BARE);
        assert_eq!(owner_form(&wrapped), BARE);
    }

    #[test]
    fn text_that_is_not_json_or_not_an_envelope_is_left_alone() {
        for text in [
            "",
            "not json",
            "[1]",
            r#"{"kind":"closed","v":{"json":"x"}}"#,
        ] {
            assert_eq!(owner_form(text), text, "{text}");
        }
    }
}
