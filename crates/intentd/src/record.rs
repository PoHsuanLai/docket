//! The router's records as almanac's events. Never content: ids, kinds, decisions. A message and
//! an episode are almanac's own bodies; everything else is an `Area { area: Docket }` payload
//! in `docket-core`'s serde form, with the things it names for cascade-forget.

use almanac_core::{KindTag, Record};
use docket_core::AuditRecord;
use prov::SpaceId;

/// The header kind of a record: `docket.<what>`, or almanac's `companion.message` and
/// `companion.episode` for the two typed bodies.
pub fn kind_tag_of(record: &AuditRecord) -> Option<KindTag> {
    let text = match record {
        AuditRecord::Call { .. } => "docket.call",
        AuditRecord::Review { .. } => "docket.review",
        AuditRecord::Confirm { .. } => "docket.confirm",
        AuditRecord::Undo { .. } => "docket.undo",
        AuditRecord::Halt { .. } => "docket.halt",
        AuditRecord::TaskPolicy { .. } => "docket.task_policy",
        AuditRecord::Breaker { .. } => "docket.breaker",
        AuditRecord::TaskStarted { .. } => "docket.task_started",
        AuditRecord::Message(_) => "companion.message",
        AuditRecord::Episode(_) => "companion.episode",
    };
    KindTag::parse(text).ok()
}

/// The almanac record for one audit record, in the Space it happened in.
pub fn record_of(record: &AuditRecord, space: &SpaceId) -> Record {
    let _ = (record, space);
    todo!(
        "record_of: Message and Episode become EventBody::Message and ::Episode; the rest become an AreaPayload (area Docket, kind from kind_tag_of, json the serde form, things the entities each call touched); actor, effect and label from the record, Cause::Event for a call a review belongs to"
    )
}
