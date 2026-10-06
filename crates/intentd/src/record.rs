//! The router's records as almanac's events. Never content: ids, kinds, decisions. A message and
//! an episode are almanac's own bodies; everything else is an `Area { area: Docket }` payload
//! in `docket-core`'s serde form, with the things it names for cascade-forget.
//!
//! What memoryd checks of a record from the router (almanac-service `record_fault`): a message
//! was sent by the record's actor, an episode's skeleton is trusted and its Space is the
//! record's, and the record's label covers the labels of the documents it carries. Each arm
//! below meets those by construction.

use almanac_core::{
    AreaPayload, AreaTag, Cause, EventBody, JsonText, KindTag, Record, ThingRole, ThingView,
    UserText,
};
use docket_core::AuditRecord;
use porter_core::AppName;
use prov::{
    Actor, AgentRef, AgentRole, Confidentiality, Effect, Integrity, Label, SessionId, Source,
    SpaceId, SystemPart, UnixSeconds,
};
use std::collections::BTreeSet;

/// The header kind of a record: `docket.<what>`, almanac's `companion.message` and
/// `companion.episode` for the two typed bodies, and `companion.session.<slug>` for a record of
/// the companion's own sessions.
pub fn kind_tag_of(record: &AuditRecord) -> Option<KindTag> {
    let text = match record {
        AuditRecord::Call { .. } => "docket.call".to_owned(),
        AuditRecord::Review { .. } => "docket.review".to_owned(),
        AuditRecord::Classified { .. } => "docket.classified".to_owned(),
        AuditRecord::Delegation { .. } => "docket.delegation".to_owned(),
        AuditRecord::Confirm { .. } => "docket.confirm".to_owned(),
        AuditRecord::Undo { .. } => "docket.undo".to_owned(),
        AuditRecord::Halt { .. } => "docket.halt".to_owned(),
        AuditRecord::TaskPolicy { .. } => "docket.task_policy".to_owned(),
        AuditRecord::Breaker { .. } => "docket.breaker".to_owned(),
        AuditRecord::TaskStarted { .. } => "docket.task_started".to_owned(),
        AuditRecord::Message(_) => "companion.message".to_owned(),
        AuditRecord::Episode(_) => "companion.episode".to_owned(),
        AuditRecord::Session { slug, .. } => format!("companion.session.{}", slug.as_str()),
    };
    KindTag::parse(&text).ok()
}

/// When the record says it happened.
fn occurred(record: &AuditRecord) -> UnixSeconds {
    match record {
        AuditRecord::Call { at, .. }
        | AuditRecord::Review { at, .. }
        | AuditRecord::Classified { at, .. }
        | AuditRecord::Delegation { at, .. }
        | AuditRecord::Confirm { at, .. }
        | AuditRecord::Undo { at, .. }
        | AuditRecord::Halt { at, .. }
        | AuditRecord::TaskPolicy { at, .. }
        | AuditRecord::Breaker { at, .. }
        | AuditRecord::TaskStarted { at, .. }
        | AuditRecord::Session { at, .. } => *at,
        AuditRecord::Message(message) => message.sent,
        AuditRecord::Episode(episode) => episode.ended,
    }
}

/// The Space the record itself names, if it does: a call, a started task, a message (the
/// sender's end) and an episode carry one. The rest are placed by the caller.
pub fn space_named_by(record: &AuditRecord) -> Option<&SpaceId> {
    match record {
        AuditRecord::Call { space, .. }
        | AuditRecord::TaskStarted { space, .. }
        | AuditRecord::Session { space, .. } => Some(space),
        AuditRecord::Message(message) => Some(&message.from.space),
        AuditRecord::Episode(episode) => Some(&episode.space),
        AuditRecord::Review { .. }
        | AuditRecord::Classified { .. }
        | AuditRecord::Delegation { .. }
        | AuditRecord::Confirm { .. }
        | AuditRecord::Undo { .. }
        | AuditRecord::Halt { .. }
        | AuditRecord::TaskPolicy { .. }
        | AuditRecord::Breaker { .. } => None,
    }
}

/// The session the log keeps for a party that is on the roster by name only: a message names
/// its sender as an `AgentRef`, and memoryd wants an `Actor` that is that party. The session is
/// a placeholder; the party is what the check reads.
fn placeholder_session() -> SessionId {
    SessionId::parse("s-0").expect("`s-0` is a valid session id")
}

/// The actor memoryd will accept as the sender of a message from `agent`.
fn actor_of(agent: &AgentRef) -> Actor {
    match agent {
        AgentRef::User => Actor::User { via: shell() },
        AgentRef::Companion => Actor::Companion {
            session: placeholder_session(),
            role: AgentRole::Planner,
        },
        AgentRef::Worker { task } => Actor::Companion {
            session: placeholder_session(),
            role: AgentRole::Worker { task: task.clone() },
        },
        AgentRef::Cua { run } => Actor::Companion {
            session: placeholder_session(),
            role: AgentRole::Cua { run: run.clone() },
        },
    }
}

fn shell() -> AppName {
    AppName::parse("org.quire.Shell").expect("`org.quire.Shell` is a valid app name")
}

fn router_app() -> AppName {
    AppName::parse("org.quire.Intents1").expect("`org.quire.Intents1` is a valid app name")
}

fn router() -> Actor {
    Actor::System {
        part: SystemPart::Router,
    }
}

/// What the router says about itself: trusted metadata, private to the Space it happened in.
fn router_label(space: &SpaceId) -> Label {
    Label {
        integrity: Integrity::Trusted,
        confidentiality: Confidentiality::Private(BTreeSet::from([space.clone()])),
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::App(router_app())]),
    }
}

/// A thing the record names, by id alone: the log keeps no titles of what an agent touched.
fn named(thing: &prov::EntityId) -> (ThingView, ThingRole) {
    (
        ThingView {
            thing: thing.clone(),
            title: UserText::new(""),
            subtitle: UserText::new(""),
        },
        ThingRole::Subject,
    )
}

/// The things a payload names, for cascade-forget: what a call touched.
fn things_of(record: &AuditRecord) -> Vec<(ThingView, ThingRole)> {
    match record {
        AuditRecord::Call { targets, .. } => targets.iter().map(named).collect(),
        _ => Vec::new(),
    }
}

/// The actor, effect and label of a record: the party the record is about, how consequential it
/// was, and what it may be shown to.
fn who_and_how(record: &AuditRecord, space: &SpaceId) -> (Actor, Effect, Label) {
    match record {
        AuditRecord::Call { actor, effect, .. } => (actor.clone(), *effect, router_label(space)),
        AuditRecord::Undo { by, .. } => (by.clone(), Effect::Read, router_label(space)),
        AuditRecord::Message(message) => (
            actor_of(&message.from.agent),
            Effect::Read,
            message.label.clone(),
        ),
        AuditRecord::Episode(episode) => {
            let label = match &episode.narrative {
                Some(narrative) => episode.skeleton.label.join(&narrative.label),
                None => episode.skeleton.label.clone(),
            };
            (actor_of(&episode.agent), Effect::Read, label)
        }
        AuditRecord::Review { .. }
        | AuditRecord::Classified { .. }
        | AuditRecord::Delegation { .. }
        | AuditRecord::Confirm { .. }
        | AuditRecord::Halt { .. }
        | AuditRecord::TaskPolicy { .. }
        | AuditRecord::Breaker { .. }
        | AuditRecord::TaskStarted { .. }
        | AuditRecord::Session { .. } => (router(), Effect::Read, router_label(space)),
    }
}

/// The almanac record for one audit record, in the Space it happened in.
///
/// `space` is where the caller says the record belongs; the Space a call, a started task, a
/// message or an episode names itself wins over it. A message and an episode are almanac's own
/// bodies; everything else is an `Area { Docket }` payload carrying the record's serde form.
pub fn record_of(record: &AuditRecord, space: &SpaceId) -> Record {
    let space = space_named_by(record).unwrap_or(space).clone();
    let (actor, effect, label) = who_and_how(record, &space);
    let body = match record {
        AuditRecord::Message(message) => EventBody::Message(message.clone()),
        AuditRecord::Episode(episode) => EventBody::Episode(episode.clone()),
        other => payload(other),
    };
    Record {
        space,
        occurred: occurred(record),
        actor,
        effect,
        label,
        body,
        cause: Cause::None,
    }
}

/// The `Area` body of a record that has no typed body of its own: `Docket`'s, in this crate's
/// serde form, or a companion session record's own JSON under `Companion`.
fn payload(record: &AuditRecord) -> EventBody {
    if let AuditRecord::Session { json, .. } = record {
        return EventBody::Area(AreaPayload {
            area: AreaTag::Companion,
            kind: kind_tag_of(record).expect("a note slug makes a valid `companion.session.` tag"),
            json: json.clone(),
            things: Vec::new(),
        });
    }
    let json = serde_json::to_string(record).unwrap_or_else(|_| "null".to_owned());
    EventBody::Area(AreaPayload {
        area: AreaTag::Docket,
        kind: kind_tag_of(record).expect("every audit record has a `docket.` kind tag"),
        json: JsonText::parse(&json).expect("serde_json writes one JSON value"),
        things: things_of(record),
    })
}
