//! What a restart reads is what the companion wrote: the records of its own sessions go to the
//! router (`Session.Note`), come back through `Session.Recall` (a body only when it is trusted)
//! and rebuild the roster and the front task. And the call that starts a task is a call like any
//! other: gated, audited, and answered with the task and the session.

use crate::support::infer::{call, words};
use crate::support::world::*;
use almanac_core::{EventRef, EventSummary, KindTag, MemoryReply, RecentEntry, ReplicaId, Seq};
use companiond::{RouterRecent, recover, replay_of};
use docket_core::*;
use prov::{Actor, Label, SpaceId, UnixSeconds};
use serde_json::json;

const START: &str = "org.quire.Companion-companion.task.start";

/// The records the router was handed for the companion's sessions, oldest first, as memory would
/// give them back from `Recent`: their kind, and their body when the label is trusted.
fn stored(w: &World) -> Vec<RecentEntry> {
    let mut seq = 0;
    w.records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Session { slug, json, .. } => {
                seq += 1;
                Some(RecentEntry {
                    summary: EventSummary {
                        event: EventRef {
                            space: space("work"),
                            replica: ReplicaId([2; 16]),
                            seq: Seq(seq),
                        },
                        occurred: UnixSeconds(1_000 + i64::try_from(seq).expect("seq")),
                        kind: KindTag::parse(&format!("companion.session.{}", slug.as_str()))
                            .expect("kind"),
                        actor: Actor::Unknown,
                        things: vec![],
                    },
                    effect: prov::Effect::Read,
                    label: Label::trusted_user(),
                    text: None,
                    body: Some(json),
                })
            }
            _ => None,
        })
        .rev()
        .collect()
}

fn slugs(w: &World) -> Vec<String> {
    w.records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Session { slug, .. } => Some(slug.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_conversation_leaves_the_records_a_restart_rebuilds_the_roster_from() {
    let mut w = world(vec![words("Hello there.")]);
    let opened = w.open("work").await;
    w.say(&opened.session, "hello").await;
    assert_eq!(slugs(&w), ["opened", "asked", "finished"]);

    let events = replay_of(&stored(&w)).expect("the records are in the form the restart reads");
    let rebuilt = agent_loop::rebuild(&events);
    assert_eq!(
        rebuilt.front, None,
        "the task finished: no front to return to"
    );
    assert_eq!(rebuilt.tasks.len(), 1);
    assert_eq!(rebuilt.tasks[0].state, RosterState::Done);
    assert_eq!(rebuilt.tasks[0].task.as_ref(), Some(&opened.task));
}

/// The record of a front conversation that was opened and never ended, as `Recent` gives it back.
fn open_front_entry() -> RecentEntry {
    let record = companion_wire::SessionRecord::Opened {
        task: task("t-9"),
        space: space("work"),
        agent: prov::AgentRef::Companion,
        parent: None,
    };
    RecentEntry {
        summary: EventSummary {
            event: EventRef {
                space: space("work"),
                replica: ReplicaId([2; 16]),
                seq: Seq(1),
            },
            occurred: UnixSeconds(900),
            kind: KindTag::parse("companion.session.opened").expect("kind"),
            actor: Actor::Unknown,
            things: vec![],
        },
        effect: prov::Effect::Read,
        label: Label::trusted_user(),
        text: None,
        body: Some(
            almanac_core::JsonText::parse(&serde_json::to_string(&record).expect("json"))
                .expect("json"),
        ),
    }
}

#[tokio::test]
async fn the_restart_reads_the_records_through_the_router_and_finds_the_front_task() {
    let mut w = world_with(vec![], vec![MemoryReply::Recent(vec![open_front_entry()])]);
    let reading = w
        .companion
        .intents
        .session_open(SessionOpen {
            space: space("work"),
            agent: prov::AgentRef::Companion,
            parent: None,
        })
        .await
        .expect("a session to read through")
        .session;
    let rebuilt = recover(
        &RouterRecent::new(&w.companion.intents, reading),
        UnixSeconds(0),
    )
    .await
    .expect("through the router");
    assert_eq!(rebuilt.front.as_ref(), Some(&task("t-9")));
    assert_eq!(rebuilt.tasks[0].state, RosterState::Working);
    let asked = w.router.seams.memory.requests();
    let almanac_core::MemoryRequest::Recent(in_space, query) = &asked[0] else {
        panic!("{asked:?}")
    };
    assert_eq!(in_space, &space("work"));
    assert_eq!(query.bodies, almanac_core::BodyMode::Json);
    let _ = &mut w;
}

#[tokio::test]
async fn restore_reads_each_space_through_a_session_of_its_own_and_leaves_nothing_behind() {
    let mut w = world_with(
        vec![],
        vec![
            // The router asks memory for its Spaces first; memory has none to add.
            MemoryReply::Spaces(vec![]),
            MemoryReply::Recent(vec![open_front_entry()]),
        ],
    );
    let resumed = w
        .companion
        .restore(&[space("work")])
        .await
        .expect("a restart")
        .expect("the front task was open: it gets a fresh session");
    assert_ne!(resumed.task, task("t-9"));
    assert!(
        w.episodes().is_empty(),
        "the session the restart read through is closed and leaves no episode: {:?}",
        w.episodes()
    );
    assert_eq!(w.companion.front.as_ref(), Some(&resumed.task));
}

#[tokio::test]
async fn starting_a_task_is_a_call_the_router_gates_and_audits_and_answers_with_task_and_session() {
    let mut w = world(vec![
        call(START, json!({ "goal": "tidy the downloads" })),
        words("on it"),
        words("done"),
    ]);
    let front = w.open("work").await;
    w.say(&front.session, "get someone to tidy up").await;

    let calls: Vec<_> = w
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { action, end, .. } => Some((action.name.to_string(), end)),
            _ => None,
        })
        .collect();
    assert!(
        calls
            .iter()
            .any(|(name, end)| name == "companion.task.start" && *end == CallEnd::Done),
        "{calls:?}"
    );
    assert!(
        w.records()
            .iter()
            .any(|r| matches!(r, AuditRecord::TaskStarted { .. })),
        "the router wrote the start of the task"
    );
}

fn summary(name: &str, state: almanac_core::SpaceState) -> almanac_core::SpaceSummary {
    almanac_core::SpaceSummary {
        id: space(name),
        state,
        vault: almanac_core::VaultKind::Plain,
        created: UnixSeconds(0),
    }
}

#[tokio::test]
async fn restore_reads_the_spaces_the_router_hands_over_and_not_only_the_configured_ones() {
    use almanac_core::{MemoryRequest, SpaceState};
    // Nothing is configured. The router lists `home` (open) and `old` (gone); the desktop is
    // always read.
    let mut w = world_with(
        vec![],
        vec![
            MemoryReply::Spaces(vec![
                summary("home", SpaceState::Open),
                summary("old", SpaceState::Gone),
            ]),
            MemoryReply::Recent(vec![]),
            MemoryReply::Recent(vec![open_front_entry()]),
        ],
    );
    let resumed = w
        .companion
        .restore(&[])
        .await
        .expect("a restart")
        .expect("the front task of `home` was open");
    assert_ne!(resumed.task, task("t-9"));
    let read: Vec<_> = w
        .router
        .seams
        .memory
        .requests()
        .into_iter()
        .filter_map(|r| match r {
            MemoryRequest::Recent(space, _) => Some(space),
            _ => None,
        })
        .collect();
    assert_eq!(read, [SpaceId::desktop(), space("home")], "`old` is gone");
}
