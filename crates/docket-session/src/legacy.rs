//! The reader for what companiond wrote before this crate: `companion_wire::SessionRecord`
//! bodies (no version), kind `companion.session.{opened,asked,replied,finished,skill_loaded,
//! closed}`. Each maps to the entry that says the same thing, or stays a typed legacy record the
//! fold counts and skips. Nothing is invented: an `Opened` has no opener (`None`), and was
//! always the native backend's.

use crate::codec::{Logged, Read, Unreadable};
use crate::entry::{BackendKind, EndCause, Opening, Seq, SessionEntry, SkillUse};
use companion_wire::SessionRecord;
use serde_json::Value;

/// The entry a legacy record becomes, if it has one.
pub fn entry_of(record: &SessionRecord) -> Option<SessionEntry> {
    match record {
        SessionRecord::Opened {
            task,
            space,
            agent,
            parent,
        } => Some(SessionEntry::Opened(Opening {
            task: task.clone(),
            space: space.clone(),
            opener: None,
            agent: Some(agent.clone()),
            backend: BackendKind::Native,
            parent: parent.clone(),
            forked_from: None,
            cwd: None,
            started_from: None,
        })),
        SessionRecord::Asked { turn, .. } => Some(SessionEntry::Turn(turn.clone())),
        SessionRecord::SkillLoaded { id, version, .. } => Some(SessionEntry::Skill(SkillUse {
            id: id.clone(),
            version: version.clone(),
        })),
        SessionRecord::Closed => Some(SessionEntry::Closed(EndCause::Closed)),
        SessionRecord::Replied { .. } | SessionRecord::Finished { .. } => None,
    }
}

/// Reads a versionless body of kind `companion.session.<slug>` (the `version` key already
/// absent, the `kind` and `v` keys as companiond wrote them).
pub(crate) fn read_legacy(slug: &str, body: Value, place: Seq) -> Logged {
    let read = match serde_json::from_value::<SessionRecord>(body) {
        Ok(record) if record.slug() != slug => Read::Unreadable(Unreadable::KindMismatch),
        Ok(record) => match entry_of(&record) {
            Some(entry) => Read::Entry(Box::new(entry)),
            None => Read::Legacy(Box::new(record)),
        },
        Err(_) => Read::Unreadable(Unreadable::Malformed),
    };
    Logged { seq: place, read }
}
