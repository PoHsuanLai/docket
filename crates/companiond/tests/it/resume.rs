//! Restart: the roster and the front task come back from what the eventlog held, and the front
//! task gets a fresh session.

use crate::support::world::*;
use agent_loop::{ReplayEvent, ReplayWhat, rebuild};
use companion_wire::SessionRecord;
use docket_core::{RosterDetail, RosterState};
use prov::{AgentRef, UnixSeconds};

fn opened(task_id: &str, in_space: &str, agent: AgentRef, at: i64) -> ReplayEvent {
    ReplayEvent {
        at: UnixSeconds(at),
        what: ReplayWhat::Session(Box::new(SessionRecord::Opened {
            task: task(task_id),
            space: space(in_space),
            agent,
            parent: None,
        })),
    }
}

#[tokio::test]
async fn a_restart_brings_back_the_roster_as_presence_and_opens_the_front_afresh() {
    let mut w = world(vec![]);
    let rebuilt = rebuild(&[
        opened("t-1", "work", AgentRef::Companion, 1),
        opened("w-2", "home", AgentRef::Worker { task: task("w-2") }, 2),
    ]);
    assert_eq!(rebuilt.front, Some(task("t-1")));

    let fresh = w
        .companion
        .resume(rebuilt)
        .await
        .expect("resumed")
        .expect("the front task is opened again");
    assert_eq!(w.companion.front, Some(fresh.task.clone()));
    assert_eq!(
        w.companion.front_task().session,
        Some(fresh.session.clone())
    );
    assert!(
        w.router
            .state
            .lock()
            .expect("lock")
            .sessions
            .contains_key(&fresh.session),
        "the router opened it"
    );

    // The person is in work: the worker in home shows as presence, working, and no more.
    let roster = w.companion.roster();
    let line = roster
        .entries
        .iter()
        .find(|l| l.agent == AgentRef::Worker { task: task("w-2") })
        .expect("the worker is back on the roster");
    assert_eq!(line.state, RosterState::Working);
    assert_eq!(line.detail, RosterDetail::PresenceOnly);
    // What the bus reads is the same.
    assert_eq!(w.companion.shared.roster(), roster);
}

#[tokio::test]
async fn a_restart_with_nothing_to_rebuild_opens_nothing() {
    let mut w = world(vec![]);
    let opened = w.companion.resume(rebuild(&[])).await.expect("resumed");
    assert_eq!(opened, None);
    assert_eq!(w.companion.front, None);
    assert!(w.companion.roster().entries.is_empty());
}
