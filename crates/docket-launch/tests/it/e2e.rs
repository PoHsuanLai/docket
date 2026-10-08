//! The backend over the real spawner, with the fake porter and the recording process seam and
//! the scripted agent of docket-acp's tests: a session start asks porter for what the agent needs,
//! and a close gives it all back.

// The scripted agent is docket-acp's; this file uses part of it.
#[allow(dead_code)]
#[path = "../../../docket-acp/tests/it/client/agent.rs"]
mod agent;

use super::support::{ENDPOINT, abs, permit};
use agent::{Act, agent as scripted, call, read, say};
use docket_acp::Ticks;
use docket_acp::client::fake::{FakeAsk, FakeFiles};
use docket_acp::client::{AcpBackend, Parts, Seams};
use docket_launch::fake::{Call, FakeAccounts, FakeProcs, Mood, Secrets};
use docket_launch::{AgentSpawn, AgentsFile, Registry};
use docket_session::{
    BackendEvent, BackendKind, Opening, ProgramName, SessionBackend, StartSession, TurnEnd,
    Workspace,
};
use docket_shell::fake::FakeSandbox;
use prov::{AgentRef, SessionId, SpaceId, TaskId, UnixSeconds};
use std::sync::Arc;

struct Fixed;
impl Ticks for Fixed {
    fn now(&self) -> UnixSeconds {
        UnixSeconds(1_760_000_000)
    }
}

struct Launched;
impl Seams for Launched {
    type Spawn = AgentSpawn<FakeAccounts, FakeProcs>;
    type Files = FakeFiles;
    type Ask = FakeAsk;
    type Sandbox = FakeSandbox;
    type Ticks = Fixed;
}

#[tokio::test]
async fn a_session_through_the_launcher_asks_porter_in_order_and_gives_it_all_back_on_close() {
    let (wire, view) = scripted(vec![vec![
        call("r", "fs/read_text_file", read("/work/app/a.txt")),
        say("done"),
        Act::Stop("end_turn"),
    ]]);
    let accounts = FakeAccounts::with(Mood::Working, Secrets::default());
    let (procs, seen) = FakeProcs::new(vec![wire]);
    let dir = tempfile::tempdir().expect("scratch");
    let spawn = AgentSpawn::new(
        permit(),
        Arc::new(AgentsFile::parse(ENDPOINT).expect("agents.toml")),
        Arc::new(accounts.clone()),
        procs,
        dir.path().to_owned(),
        abs("/opt/docket/docket-net-forward"),
        Registry::default(),
    );
    let files = FakeFiles::new();
    files.put(&abs("/work/app/a.txt"), "text");
    let (sandbox, _) = FakeSandbox::ready(Vec::new());
    let program = ProgramName::parse("claude-code").expect("program");
    let session = SessionId::parse("s-1").expect("session");
    let mut backend = AcpBackend::<Launched>::new(Parts {
        program: program.clone(),
        session: session.clone(),
        spawn,
        files,
        ask: FakeAsk::new(Vec::new()),
        sandbox,
        ticks: Fixed,
        grants: Vec::new(),
    });
    backend
        .start(StartSession {
            session,
            opening: Opening {
                task: TaskId::parse("t-1").expect("task"),
                space: SpaceId::desktop(),
                opener: None,
                agent: Some(AgentRef::Companion),
                backend: BackendKind::Acp(program),
                parent: None,
                forked_from: None,
                started_from: None,
                cwd: Some(Workspace::parse("/work/app").expect("cwd")),
            },
        })
        .await
        .expect("start");
    assert_eq!(seen.runs().len(), 1);
    backend
        .turn(docket_core::UserTurn {
            id: docket_core::TurnId(1),
            text: "go".to_owned(),
            at: UnixSeconds(1),
            from: docket_core::TurnSource::Launcher,
            via: docket_core::TurnVia::Typed,
        })
        .await
        .expect("turn");
    let mut events = Vec::new();
    while let Some(event) = backend.next_event().await {
        events.push(event);
    }
    assert!(matches!(
        events.last(),
        Some(BackendEvent::TurnEnd(TurnEnd::Done))
    ));
    assert_eq!(view.reply("r").expect("read")["content"], "text");

    backend.close().await;
    assert_eq!(
        accounts.calls(),
        [
            Call::Begin("acp-s-1".into()),
            Call::Open("claude-code".into(), "anthropic-main".into()),
            Call::Close("ep-1".into()),
            Call::End("acp-s-1".into()),
        ]
    );
    assert!(seen.killed(0));
}
