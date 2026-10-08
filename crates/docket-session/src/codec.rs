//! The stored form: `{"version":1,"seq":n,"kind":"turn","v":{...}}`, written as an eventlog
//! `Area { Companion }` body of kind `companion.session.<slug>`. The version is explicit and read
//! first, so a body from a future docket is `Unreadable::UnknownVersion`, never a guess. A body
//! with no version is a record companiond wrote before this crate (`legacy`).

use crate::entry::{Seq, SessionEntry};
use crate::legacy::read_legacy;
use companion_wire::{SESSION_KIND_PREFIX, SessionRecord};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The version of the stored form this crate writes and reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EntryVersion(pub u32);

/// The one version there is.
pub const CURRENT: EntryVersion = EntryVersion(1);

/// The kind tag of an entry in the eventlog.
pub fn kind_tag(slug: &str) -> String {
    format!("{SESSION_KIND_PREFIX}.{slug}")
}

#[derive(Serialize)]
struct Envelope<'a> {
    version: EntryVersion,
    seq: Seq,
    #[serde(flatten)]
    entry: &'a SessionEntry,
}

/// An entry ready for the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Encoded {
    /// The kind tag, `companion.session.<slug>`.
    pub kind: String,
    /// The body.
    pub json: String,
}

/// Why an entry could not be written.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a session entry could not be written as JSON")]
pub struct EncodeFault;

/// An entry at position `seq` in the stored form.
pub fn encode(seq: Seq, entry: &SessionEntry) -> Result<Encoded, EncodeFault> {
    let envelope = Envelope {
        version: CURRENT,
        seq,
        entry,
    };
    Ok(Encoded {
        kind: kind_tag(entry.slug()),
        json: serde_json::to_string(&envelope).map_err(|_| EncodeFault)?,
    })
}

/// Why a body could not be read. Each one fails closed in `resume_plan`: what cannot be read
/// might have been a taint, a policy narrowing or a close.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Unreadable {
    /// Not JSON, or not an object.
    NotJson,
    /// A version this crate does not know.
    UnknownVersion(EntryVersion),
    /// A version that is not a number.
    BadVersion,
    /// A version it knows, a kind it does not.
    UnknownKind,
    /// The kind tag and the body disagree.
    KindMismatch,
    /// The body does not fit its kind.
    Malformed,
}

/// What a body is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum Read {
    /// An entry of this crate's form, or a legacy record that maps onto one.
    Entry(Box<SessionEntry>),
    /// A legacy record with no entry to become (`Replied`, `Finished`): kept, not folded.
    Legacy(Box<SessionRecord>),
    /// A body that could not be read.
    Unreadable(Unreadable),
}

/// One row of a session's log, in log order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Logged {
    /// Its position: the stored `seq`, or the row's place in the log for a legacy body.
    pub seq: Seq,
    /// What it holds.
    pub read: Read,
}

const KINDS: [&str; 11] = [
    "opened", "turn", "policy", "call", "step", "handle", "taint", "breaker", "budget", "skill",
    "closed",
];

/// Reads a stored body of kind `companion.session.<slug>`; `place` is the row's position in the
/// log, used when the body carries none.
pub fn decode(slug: &str, json: &str, place: Seq) -> Logged {
    let unread = |why| Logged {
        seq: place,
        read: Read::Unreadable(why),
    };
    let Ok(Value::Object(mut body)) = serde_json::from_str::<Value>(json) else {
        return unread(Unreadable::NotJson);
    };
    let Some(version) = body.remove("version") else {
        return read_legacy(slug, Value::Object(body), place);
    };
    let Some(version) = version.as_u64().and_then(|v| u32::try_from(v).ok()) else {
        return unread(Unreadable::BadVersion);
    };
    if EntryVersion(version) != CURRENT {
        return unread(Unreadable::UnknownVersion(EntryVersion(version)));
    }
    current(slug, body, place)
}

fn current(slug: &str, mut body: Map<String, Value>, place: Seq) -> Logged {
    let seq = body
        .remove("seq")
        .and_then(|s| serde_json::from_value::<Seq>(s).ok())
        .unwrap_or(place);
    let unread = |why| Logged {
        seq,
        read: Read::Unreadable(why),
    };
    let known = body
        .get("kind")
        .and_then(Value::as_str)
        .is_some_and(|k| KINDS.contains(&k));
    if !known {
        return unread(Unreadable::UnknownKind);
    }
    match serde_json::from_value::<SessionEntry>(Value::Object(body)) {
        Ok(entry) if entry.slug() == slug => Logged {
            seq,
            read: Read::Entry(Box::new(entry)),
        },
        Ok(_) => unread(Unreadable::KindMismatch),
        Err(_) => unread(Unreadable::Malformed),
    }
}
