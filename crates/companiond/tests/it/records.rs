//! What a restart reads is what the companion wrote: the records of its own sessions go to the
//! router (`Session.Note`), come back through `Session.Recall` (a body only when it is trusted)
//! and rebuild the roster and the front task. And the call that starts a task is a call like any
//! other: gated, audited, and answered with the task and the session.

use crate::support::infer::{call, words};
use crate::support::world::*;
use almanac_core::MemoryReply;
use companiond::{RouterLog, stored_events};
use docket_core::*;
use prov::{SpaceId, UnixSeconds};
use serde_json::json;

const START: &str = "org.quire.Companion-companion.task.start";

/// The sessions the router stores, as a restarted companion reads them.
async fn stored(w: &World) -> Vec<agent_loop::ReplayEvent> {
    stored_events(&RouterLog::reading(&w.companion.intents))
        .await
        .expect("the router lists its sessions")
        .expect("it stores some")
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
async fn a_conversation_leaves_the_rows_a_restart_rebuilds_the_roster_from() {
    let mut w = world(vec![words("Hello there.")]);
    let opened = w.open("work").await;
    w.say(&opened.session, "hello").await;
    assert_eq!(slugs(&w), ["opened", "asked", "finished"]);

    let events = stored(&w).await;
    let rebuilt = agent_loop::rebuild(&events);
    // The router's log holds the opening and the person's turn. That the turn finished is a note
    // in the eventlog (a legacy row of the stored view in a real deployment, see `stored_roster`
    // tests) and an episode: this fake log holds neither, so the session reads as open.
    assert_eq!(rebuilt.front.as_ref(), Some(&opened.task));
    assert_eq!(rebuilt.tasks.len(), 1);
    assert_eq!(rebuilt.tasks[0].state, RosterState::Working);
    assert_eq!(rebuilt.tasks[0].task.as_ref(), Some(&opened.task));
}

#[tokio::test]
async fn an_editors_host_writes_no_roster_notes() {
    let mut w = world(vec![words("Hello there.")]);
    w.companion = w.restarted().without_notes();
    let opened = w.open("work").await;
    w.say(&opened.session, "hello").await;
    assert_eq!(slugs(&w), Vec::<String>::new());
}

/// A legacy session record as the router is handed it.
fn as_note(record: &companion_wire::SessionRecord) -> NoteAsk {
    NoteAsk::Record(SessionNote {
        slug: NoteSlug::parse(record.slug()).expect("slug"),
        json: almanac_core::JsonText::parse(&serde_json::to_string(record).expect("json"))
            .expect("json"),
    })
}

#[tokio::test]
async fn a_restart_finds_the_front_task_in_the_sessions_the_router_stores() {
    let mut w = world(vec![]);
    let opened = w.open("work").await;
    let rebuilt = agent_loop::rebuild(&stored(&w).await);
    assert_eq!(rebuilt.front.as_ref(), Some(&opened.task));
    assert_eq!(rebuilt.tasks[0].state, RosterState::Working);
}

#[tokio::test]
async fn restore_rebuilds_from_the_stored_sessions_and_opens_the_front_afresh() {
    let mut w = world_with(
        vec![],
        vec![
            // The router asks memory for its Spaces first; memory has none to add.
            MemoryReply::Spaces(vec![]),
            MemoryReply::Recent(vec![]),
        ],
    );
    let before = w.open("work").await;
    let mut fresh = w.restarted();
    let resumed = fresh
        .restore(&[space("work")])
        .await
        .expect("a restart")
        .expect("the front task was open: it gets a fresh session");
    assert_ne!(resumed.task, before.task);
    assert_eq!(fresh.front.as_ref(), Some(&resumed.task));
    assert!(
        w.episodes().is_empty(),
        "the session the restart read through is closed and leaves no episode: {:?}",
        w.episodes()
    );
}

#[tokio::test]
async fn sessions_an_editor_opened_are_not_on_the_roster_or_the_front_after_a_restart() {
    let mut w = world_with(
        vec![],
        vec![MemoryReply::Spaces(vec![]), MemoryReply::Recent(vec![])],
    );
    let editors = w.open_for_editor("work").await;
    // Its legacy notes (an older host wrote them) do not bring it back either.
    let note = companion_wire::SessionRecord::Opened {
        task: editors.task.clone(),
        space: space("work"),
        agent: prov::AgentRef::Companion,
        parent: None,
    };
    let _ = w
        .companion
        .intents
        .session_note(editors.session.clone(), as_note(&note))
        .await;
    let mut fresh = w.restarted();
    let resumed = fresh.restore(&[space("work")]).await.expect("a restart");
    assert_eq!(resumed, None, "an editor's session is not the front");
    assert_eq!(fresh.front, None);
    assert!(fresh.roster().entries.is_empty());
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
            MemoryReply::Recent(vec![]),
        ],
    );
    let before = w.open("home").await;
    let mut fresh = w.restarted();
    let resumed = fresh
        .restore(&[])
        .await
        .expect("a restart")
        .expect("the front task of `home` was open");
    assert_ne!(resumed.task, before.task);
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
