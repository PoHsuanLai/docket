//! The backend under test, wired to fakes, and the helpers every test here uses.

use super::agent::{Act, View, agent};
use crate::support::{Fixed, app};
use docket_acp::Answer;
use docket_acp::client::fake::{FakeAsk, FakeFiles, FakeSpawn, SpawnSeen};
use docket_acp::client::{AcpBackend, OsFiles, Parts, Seams};
use docket_core::{AbsPath, StandingGrant, TurnId, TurnSource, TurnVia, UserTurn};
use docket_session::{
    BackendEvent, BackendKind, Opening, ProgramName, SessionBackend, StartSession, Workspace,
};
use docket_shell::fake::{FakeSandbox, Script, Seen};
use prov::{AgentRef, SessionId, SpaceId, TaskId, UnixSeconds};

pub struct Fakes;

impl Seams for Fakes {
    type Spawn = FakeSpawn;
    type Files = FakeFiles;
    type Ask = FakeAsk;
    type Sandbox = FakeSandbox;
    type Ticks = Fixed;
}

pub struct Real;

impl Seams for Real {
    type Spawn = FakeSpawn;
    type Files = OsFiles;
    type Ask = FakeAsk;
    type Sandbox = FakeSandbox;
    type Ticks = Fixed;
}

pub const CWD: &str = "/work/app";

pub fn abs(text: &str) -> AbsPath {
    AbsPath::parse(text).expect("abs")
}

pub fn program() -> ProgramName {
    ProgramName::parse("claude-code").expect("program")
}

pub fn session() -> SessionId {
    SessionId::parse("s-1").expect("session")
}

pub fn opening(cwd: &str) -> Opening {
    Opening {
        task: TaskId::parse("t-1").expect("task"),
        space: SpaceId::desktop(),
        opener: Some(app("org.quire.Acp")),
        agent: Some(AgentRef::Companion),
        backend: BackendKind::Acp(program()),
        parent: None,
        forked_from: None,
        cwd: Some(Workspace::parse(cwd).expect("cwd")),
    }
}

pub fn turn(n: u64, text: &str) -> UserTurn {
    UserTurn {
        id: TurnId(n),
        text: text.to_owned(),
        at: UnixSeconds(1_760_000_000),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    }
}

/// Everything a test looks at besides the backend.
pub struct Rig {
    pub backend: AcpBackend<Fakes>,
    pub files: FakeFiles,
    pub ask: FakeAsk,
    pub sandbox: Seen,
    pub spawned: SpawnSeen,
    pub agent: View,
}

#[derive(Default)]
pub struct Setup {
    pub turns: Vec<Vec<Act>>,
    pub answers: Vec<Answer>,
    pub scripts: Vec<Script>,
    pub grants: Vec<StandingGrant>,
}

/// A started backend: the handshake is done and it waits for a turn.
pub async fn started(setup: Setup) -> Rig {
    let (wire, view) = agent(setup.turns);
    let (spawn, spawned) = FakeSpawn::new(vec![wire]);
    let files = FakeFiles::new();
    let ask = FakeAsk::new(setup.answers);
    let (sandbox, seen) = FakeSandbox::ready(setup.scripts);
    let mut backend = AcpBackend::new(Parts {
        program: program(),
        session: session(),
        spawn,
        files: files.clone(),
        ask: ask.clone(),
        sandbox,
        ticks: Fixed,
        grants: setup.grants,
    });
    backend
        .start(StartSession {
            session: session(),
            opening: opening(CWD),
        })
        .await
        .expect("start");
    Rig {
        backend,
        files,
        ask,
        sandbox: seen,
        spawned,
        agent: view,
    }
}

/// Gives the backend a turn and pulls events to the end of it.
pub async fn run_turn(backend: &mut AcpBackend<Fakes>, text: &str) -> Vec<BackendEvent> {
    backend.turn(turn(1, text)).await.expect("turn");
    drain(backend).await
}

pub async fn drain<B: SessionBackend>(backend: &mut B) -> Vec<BackendEvent> {
    let mut events = Vec::new();
    while let Some(event) = backend.next_event().await {
        events.push(event);
    }
    events
}

/// The tool-call fields a permission request carries.
pub fn tool(kind: &str, title: &str, paths: &[&str], raw: serde_json::Value) -> serde_json::Value {
    let locations: Vec<_> = paths
        .iter()
        .map(|p| serde_json::json!({"path": p}))
        .collect();
    serde_json::json!({
        "sessionId": super::agent::AGENT_SESSION,
        "toolCall": {
            "toolCallId": "tc-1", "kind": kind, "title": title,
            "locations": locations, "rawInput": raw
        },
        "options": [
            {"optionId": "a-always", "name": "Always", "kind": "allow_always"},
            {"optionId": "a-once", "name": "Once", "kind": "allow_once"},
            {"optionId": "r-once", "name": "No", "kind": "reject_once"}
        ]
    })
}

pub fn read(path: &str) -> serde_json::Value {
    serde_json::json!({"sessionId": super::agent::AGENT_SESSION, "path": path})
}

pub fn write(path: &str, content: &str) -> serde_json::Value {
    serde_json::json!({"sessionId": super::agent::AGENT_SESSION, "path": path, "content": content})
}

pub fn selected(reply: &Result<serde_json::Value, serde_json::Value>) -> Option<String> {
    let value = reply.as_ref().ok()?;
    value["outcome"]["optionId"].as_str().map(str::to_owned)
}
