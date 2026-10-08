//! The rules every `SessionBackend` keeps (docket-session's contract), run against `AcpBackend`.

use super::agent::{Act, agent, call, say};
use super::rig::{Fakes, abs, opening, program, read, session, turn};
use crate::support::Fixed;
use docket_acp::client::fake::{FakeAsk, FakeFiles, FakeSpawn};
use docket_acp::client::{AcpBackend, Parts};
use docket_session::contract::{self, Harness};
use docket_session::{SessionBackend, StartSession};
use docket_shell::fake::FakeSandbox;

struct Acp {
    files: FakeFiles,
    /// Reads before the backend under test began: `ran` counts from here.
    base: usize,
}

impl Harness for Acp {
    type Backend = AcpBackend<Fakes>;

    async fn backend(&mut self) -> Self::Backend {
        // Every backend of the contract plays the same turn: one read, words, the end. A second
        // turn plays the default (an immediate end).
        let script = vec![
            call("r", "fs/read_text_file", read("/work/app/a.txt")),
            say("read it"),
            Act::Stop("end_turn"),
        ];
        let (wire, _view) = agent(vec![script]);
        let (spawn, _seen) = FakeSpawn::new(vec![wire]);
        let (sandbox, _) = FakeSandbox::ready(Vec::new());
        self.files.put(&abs("/work/app/a.txt"), "text");
        self.base = self.files.reads();
        let mut backend = AcpBackend::new(Parts {
            program: program(),
            session: session(),
            spawn,
            files: self.files.clone(),
            ask: FakeAsk::new(Vec::new()),
            sandbox,
            ticks: Fixed,
            grants: Vec::new(),
        });
        backend
            .start(StartSession {
                session: session(),
                opening: opening("/work/app"),
            })
            .await
            .expect("start");
        backend
    }

    fn turn(&self, n: u64) -> docket_core::UserTurn {
        turn(n, "go")
    }

    fn ran(&self, _backend: &Self::Backend) -> usize {
        self.files.reads() - self.base
    }

    async fn settle(&mut self) {
        for _ in 0..16 {
            tokio::task::yield_now().await;
        }
    }

    fn stalls(&self) -> usize {
        1
    }
}

fn harness() -> Acp {
    Acp {
        files: FakeFiles::new(),
        base: 0,
    }
}

#[tokio::test]
async fn it_runs_a_turn() {
    let mut h = harness();
    contract::runs_a_turn(&mut h).await;
}

#[tokio::test]
async fn a_stop_before_the_next_pull_stops_the_call() {
    let mut h = harness();
    contract::a_stop_before_the_next_pull_stops_the_call(&mut h).await;
}

#[tokio::test]
async fn a_call_waits_for_the_pull_after_its_announcement() {
    let mut h = harness();
    contract::a_call_waits_for_the_pull_after_its_announcement(&mut h).await;
}

#[tokio::test]
async fn a_dropped_pull_loses_and_repeats_nothing() {
    let mut h = harness();
    contract::a_dropped_pull_loses_and_repeats_nothing(&mut h).await;
}
