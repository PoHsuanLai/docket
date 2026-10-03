//! Restart. companiond keeps no state of its own: the roster and the front task are rebuilt from
//! the records the eventlog holds (its own session records, the router's messages and the
//! episodes), read through memory's `Recent` with `BodyMode::Json`. Each entry's body is the
//! owner's serde form, so an entry that does not parse is a fault, never a guess.

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
    let text = body.as_str();
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
    let record: serde_json::Value = serde_json::from_str(body.as_str()).map_err(malformed)?;
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

/// Rebuilds the roster and the front task from what `source` holds since `since`.
pub async fn recover<S: RecentSource>(
    source: &S,
    since: UnixSeconds,
) -> Result<Rebuilt, ReplayFault> {
    let entries = source.recent(restart_query(since)).await?;
    Ok(rebuild(&replay_of(&entries)?))
}
