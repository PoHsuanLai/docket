//! The rules every `SessionBackend` keeps (docket-session's contract), run against `AcpBackend`
//! with its calls going through the real router.

use super::agent::{Act, call, say};
use super::rig::{Fakes, Setup, abs, opening, read, wired_over};
use docket_acp::client::AcpBackend;
use docket_acp::client::fake::FakeFiles;
use docket_acp::client::{Court, OpenAgent};
use docket_core::SheetSurface;
use docket_session::contract::{self, Harness};
use docket_session::{SessionBackend, StartSession};

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
        self.files.put(&abs("/work/app/a.txt"), "text");
        self.base = self.files.reads();
        let mut wired = wired_over::<Fakes>(
            self.files.clone(),
            Setup {
                turns: vec![script],
                ..Setup::default()
            },
        );
        let opened = opening("/work/app");
        let session = wired
            .court
            .open(OpenAgent {
                program: super::rig::program(),
                cwd: opened.cwd.clone().expect("cwd"),
                sheets: SheetSurface::Desktop,
                space: opened.space.clone(),
                label: opened.label.clone(),
                rewind: Default::default(),
            })
            .await
            .expect("the router opens the session");
        wired
            .backend
            .start(StartSession {
                session,
                opening: opened,
            })
            .await
            .expect("start");
        wired.backend
    }

    fn turn(&self, n: u64) -> docket_core::UserTurn {
        super::rig::turn(n, "go")
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
