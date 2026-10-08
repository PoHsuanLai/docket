//! A call waiting on the router when the backend is pulled again or cancelled: it holds one
//! staged request, and a cancel takes the hold away. A court that never answers stands for the
//! router waiting on the person's sheet; nothing here waits on a clock.

use super::agent::{Auth, agent_signing, call};
use super::rig::{CWD, opening, turn, write};
use bulkhead::Network;
use bulkhead::fake::FakeSandbox;
use docket_acp::client::fake::{FakeFiles, FakeSpawn};
use docket_acp::client::{
    AcpBackend, AgentCall, Court, CourtFault, EdgeFault, OpenAgent, Parts, Performer, Ruled, Seams,
    StageId, ToolsOffer,
};
use docket_core::ValidManifest;
use docket_session::{SessionBackend, StartSession};
use futures_util::future::pending;
use prov::SessionId;
use std::sync::{Arc, Mutex, MutexGuard};
use tokio::sync::Notify;

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A router that takes every call and never rules on it, and says when one arrives.
#[derive(Clone, Default)]
struct Silent {
    stages: Arc<Mutex<Vec<StageId>>>,
    asked: Arc<Notify>,
}

impl Court for Silent {
    async fn open(&mut self, _open: OpenAgent) -> Result<SessionId, CourtFault> {
        Err(CourtFault::Unavailable)
    }

    async fn turn(&mut self, _session: &SessionId, _text: &str) -> Result<(), CourtFault> {
        Ok(())
    }

    async fn call(&mut self, _session: &SessionId, _n: u64, call: &AgentCall) -> Ruled {
        if let AgentCall::Write { stage, .. } = call {
            locked(&self.stages).push(stage.clone());
        }
        self.asked.notify_one();
        pending().await
    }

    async fn registry(&mut self) -> Option<Vec<ValidManifest>> {
        None
    }

    async fn close(&mut self, _session: &SessionId) {}
}

struct Silence;

impl Seams for Silence {
    type Spawn = FakeSpawn;
    type Files = FakeFiles;
    type Court = Silent;
    type Sandbox = FakeSandbox;
}

struct Waiting {
    backend: AcpBackend<Silence>,
    performer: Performer<FakeFiles, FakeSandbox>,
    court: Silent,
}

fn backend(tools: Option<ToolsOffer>) -> Waiting {
    let (wire, _view) = agent_signing(
        vec![vec![call(
            "w",
            "fs/write_text_file",
            write("/work/app/a.rs", "fn main() {}"),
        )]],
        Auth::Open,
    );
    let (spawn, _seen) = FakeSpawn::new(vec![wire]);
    let (sandbox, _seen) = FakeSandbox::ready(Vec::new());
    let performer = Performer::with_network(FakeFiles::new(), sandbox, Network::Off);
    let court = Silent::default();
    let backend = AcpBackend::<Silence>::new(Parts {
        program: super::rig::program(),
        session: SessionId::parse("s-1").expect("session"),
        spawn,
        performer: performer.clone(),
        court: court.clone(),
        tools,
    });
    Waiting {
        backend,
        performer,
        court,
    }
}

async fn start(w: &mut Waiting) {
    w.backend
        .start(StartSession {
            session: SessionId::parse("s-1").expect("session"),
            opening: opening(CWD),
        })
        .await
        .expect("start");
}

/// A started backend whose agent has asked for a write, announced but not yet ruled on.
async fn waiting() -> Waiting {
    let mut w = backend(None);
    start(&mut w).await;
    w.backend.turn(turn(1, "go")).await.expect("turn");
    let announced = w.backend.next_event().await;
    assert!(announced.is_some(), "the call is announced first");
    w
}

/// Pulls until the router has been asked, then drops the pull, as the host does when a sheet
/// arrives first.
async fn pull_until_asked(waiting: &mut Waiting) {
    tokio::select! {
        biased;
        event = waiting.backend.next_event() => panic!("the call cannot finish: {event:?}"),
        () = waiting.court.asked.notified() => {}
    }
}

/// Why: a pull the router's wait cut short is made again; it must find the request it already
/// staged, not stage a second copy that nothing will ever spend.
#[tokio::test]
async fn pulling_again_while_the_router_waits_stages_the_call_once() {
    let mut w = waiting().await;
    pull_until_asked(&mut w).await;
    pull_until_asked(&mut w).await;
    let stages = locked(&w.court.stages).clone();
    assert_eq!(stages.len(), 2, "the router was asked on both pulls");
    assert_eq!(stages[0], stages[1], "and named the same held request");
}

/// Why: after the agent is told "cancelled" the call must not run, even if the router rules yes
/// later: the held request is gone, so `perform` finds nothing to run.
#[tokio::test]
async fn a_cancel_drops_the_held_request_so_nothing_runs_after_it() {
    let mut w = waiting().await;
    pull_until_asked(&mut w).await;
    let stage = locked(&w.court.stages)[0].clone();
    assert!(w.performer.holds(&stage), "held while the router waits");
    w.backend.cancel().await;
    assert!(!w.performer.holds(&stage), "a cancel takes the hold away");
}

/// Why: a tool edge that was offered and could not be made leaves the agent without the
/// desktop's actions; the backend keeps the reason so the host can say so.
#[tokio::test]
async fn a_tool_edge_that_cannot_be_made_is_reported_not_swallowed() {
    let offer = ToolsOffer {
        run_dir: "/no/such/run/dir".into(),
        bridge: super::rig::abs("/usr/bin/bridge"),
    };
    let mut w = backend(Some(offer));
    start(&mut w).await;
    assert_eq!(w.backend.edge_fault(), Some(EdgeFault::Io));
    let mut plain = backend(None);
    start(&mut plain).await;
    assert_eq!(
        plain.backend.edge_fault(),
        None,
        "none offered, none failed"
    );
}
