//! The native backend against the rules every backend keeps (`docket_session::contract`), the
//! ones the fake passes in `docket-session`. Every planner answer is late, so a pull is pending on
//! its first poll and a dropped pull drops a loop that is in the middle of something.

use crate::support::infer::{Say, ScriptedInfer, call, words};
use crate::support::*;
use docket_client::InProcess;
use docket_core::{SessionOpen, TurnId, TurnSource, TurnVia, UserTurn};
use docket_fake::FakeSeams;
use docket_session::contract::{self, Harness};
use docket_session::{Opening, SessionBackend, StartSession};
use docket_tasks::{NativeBackend, Quiet};
use futures_util::lock::Mutex;
use prov::{AgentRef, UnixSeconds};
use serde_json::json;
use std::sync::Arc;

type Backend = NativeBackend<ScriptedInfer, InProcess<FakeSeams>, Still, Quiet>;

fn script() -> Vec<Say> {
    vec![
        Say::Late(Box::new(call(ARCHIVE, json!({ "target": [thread("t2")] })))),
        Say::Late(Box::new(words("Archived."))),
        Say::Late(Box::new(call(ARCHIVE, json!({ "target": [thread("t2")] })))),
        Say::Late(Box::new(words("Archived."))),
    ]
}

#[derive(Default)]
struct Native {
    world: Option<World>,
}

impl Harness for Native {
    type Backend = Backend;

    async fn backend(&mut self) -> Backend {
        let world = World::new(script());
        let companion = companion(&world.router, &world.infer);
        let shared = companion.shared.clone();
        let core = Arc::new(Mutex::new(companion));
        let opened = core
            .lock()
            .await
            .open(SessionOpen {
                space: work(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
            })
            .await
            .expect("open");
        let mut backend = NativeBackend::new(core, shared, opened.session.clone());
        backend
            .start(StartSession {
                session: opened.session,
                opening: Opening {
                    task: opened.task,
                    space: work(),
                    opener: None,
                    agent: None,
                    backend: docket_session::BackendKind::Native,
                    parent: None,
                    forked_from: None,
                    cwd: None,
                    started_from: None,
                },
            })
            .await
            .expect("start");
        self.world = Some(world);
        backend
    }

    fn turn(&self, n: u64) -> UserTurn {
        UserTurn {
            id: TurnId(n),
            text: "archive the digest".into(),
            at: UnixSeconds(1_000),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        }
    }

    fn ran(&self, _backend: &Backend) -> usize {
        self.world.as_ref().map_or(0, World::performed)
    }

    async fn settle(&mut self) {
        for _ in 0..50 {
            tokio::task::yield_now().await;
        }
    }

    fn stalls(&self) -> usize {
        1
    }
}

#[tokio::test]
async fn runs_a_turn() {
    contract::runs_a_turn(&mut Native::default()).await;
}

#[tokio::test]
async fn a_stop_before_the_next_pull_stops_the_call() {
    contract::a_stop_before_the_next_pull_stops_the_call(&mut Native::default()).await;
}

#[tokio::test]
async fn a_call_waits_for_the_pull_after_its_announcement() {
    contract::a_call_waits_for_the_pull_after_its_announcement(&mut Native::default()).await;
}

#[tokio::test]
async fn a_dropped_pull_loses_and_repeats_nothing() {
    contract::a_dropped_pull_loses_and_repeats_nothing(&mut Native::default()).await;
}
