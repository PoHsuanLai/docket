//! The backend under test, wired to the real router over the fakes, and the helpers every test
//! here uses. The router is the real one (policy point, reviewers, taint, breaker, budgets,
//! audit); what is faked is the world around it: the agent process, the files, the sandbox, the
//! person's answers, and the models.

use super::agent::{Act, View, agent};
use crate::support::app;
use docket_acp::client::fake::{FakeFiles, FakeSpawn, SpawnSeen};
use docket_acp::client::{AcpBackend, AgentHost, Fallback, IntentsCourt, Parts, Performer, Seams};
use docket_client::InProcess;
use docket_core::{
    AbsPath, ActionMatch, AgentConfig, CallerId, CallerRole, ConfirmAnswer, ConfirmEnd,
    ConfirmRequest, LabelText, StandingGrant, TaskPolicy, TaskPolicyState, TurnId, TurnSource,
    TurnVia, UserTurn, acp_agent_app,
};
use docket_fake::{
    FixedClock, ScriptedConfirmer, ScriptedReviewer, ScriptedWriter, fake_router_over,
};
use docket_inapp::{EditorDesk, SheetConfirmer};
use docket_router::GrantStore;
use docket_session::fake::MemoryLog;
use docket_session::{
    BackendEvent, BackendKind, Opening, ProgramName, SessionHost, SheetChoice, TurnEnd, Workspace,
};
use docket_shell::fake::{FakeSandbox, Script, Seen};
use docket_shell::{Network, NetworkMode};
use porter_core::{AppId, Count, Isolation};
use prov::{AgentRef, Effect, SessionId, SpaceId, TaskId, UnixSeconds};
use std::collections::BTreeSet;
use std::sync::Arc;

pub const CWD: &str = "/work/app";
pub const HOST: &str = "org.quire.AcpAgent";

pub fn abs(text: &str) -> AbsPath {
    AbsPath::parse(text).expect("abs")
}

pub fn program() -> ProgramName {
    ProgramName::parse("claude-code").expect("program")
}

pub fn host_caller() -> CallerId {
    CallerId {
        app: AppId {
            name: app(HOST),
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::AcpAgent]),
    }
}

pub fn opening(cwd: &str) -> Opening {
    Opening {
        task: TaskId::parse("t-1").expect("task"),
        space: SpaceId::parse("work").expect("space"),
        opener: Some(app(HOST)),
        agent: Some(AgentRef::Companion),
        backend: BackendKind::Acp(program()),
        parent: None,
        forked_from: None,
        started_from: None,
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

/// The person says yes, this once.
pub fn once() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: porter_core::consent::GrantScope::Once,
        receipt: receipt(),
    }
}

/// The person says yes, always (honoured only where the sheet offered it).
pub fn always() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: porter_core::consent::GrantScope::Always,
        receipt: receipt(),
    }
}

/// The person says no.
pub fn no() -> ConfirmAnswer {
    ConfirmAnswer::Ended(ConfirmEnd::Refused)
}

fn receipt() -> prov::ConfirmReceipt {
    prov::ConfirmReceipt {
        id: prov::ConfirmId::parse("c-1").expect("id"),
        input: prov::InputProof::HardwareSeat,
        at: UnixSeconds(1),
        covers: prov::Confidentiality::Secret,
    }
}

/// What the task policy the person's words derive covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Policy {
    /// The whole pseudo-app, up to destructive.
    #[default]
    Wide,
    /// Nothing: the writer fails, so every non-read call is outside the task.
    None,
}

pub struct Setup {
    pub turns: Vec<Vec<Act>>,
    /// The person's answers on the desktop's sheet, in order; then they dismiss it.
    pub answers: Vec<ConfirmAnswer>,
    pub scripts: Vec<Script>,
    pub fallback: Fallback,
    pub policy: Policy,
    pub reviewer: ScriptedReviewer,
    /// Standing grants already held in docket's store.
    pub held: Vec<StandingGrant>,
    pub config: AgentConfig,
    /// The network the agent's commands run with.
    pub network: Network,
    /// The network the agent process itself runs with (R12).
    pub agent_network: NetworkMode,
}

impl Default for Setup {
    fn default() -> Self {
        Self {
            turns: Vec::new(),
            answers: Vec::new(),
            scripts: Vec::new(),
            fallback: Fallback::Off,
            policy: Policy::Wide,
            reviewer: ScriptedReviewer::always_allow(),
            held: Vec::new(),
            config: AgentConfig::default(),
            network: Network::Off,
            agent_network: NetworkMode::None,
        }
    }
}

fn wide_policy() -> TaskPolicy {
    TaskPolicy {
        task: TaskId::parse("t-1").expect("task"),
        space: SpaceId::parse("work").expect("space"),
        from: Vec::new(),
        actions: BTreeSet::from([ActionMatch::AppUpTo(
            acp_agent_app().expect("app"),
            Effect::Destructive,
        )]),
        kinds: BTreeSet::new(),
        ceiling: Effect::Destructive,
        max_count: Count(100),
        recipients: Vec::new(),
        destinations: Vec::new(),
        paths: Vec::new(),
        expires: UnixSeconds(i64::MAX),
        rationale: LabelText::parse("test policy").expect("words"),
        state: TaskPolicyState::Active,
    }
}

/// Everything a test looks at besides the host.
pub struct Rig<X: Seams<Court = TheCourt, Sandbox = FakeSandbox, Spawn = FakeSpawn>> {
    pub host: AgentHost<X, EditorDesk>,
    pub router: Arc<World>,
    pub session: SessionId,
    pub desk: EditorDesk,
    pub sandbox: Seen,
    pub spawned: SpawnSeen,
    pub agent: View,
    pub performer: Performer<X::Files, FakeSandbox>,
    pub court: TheCourt,
}

impl<X: Seams<Court = TheCourt, Sandbox = FakeSandbox, Spawn = FakeSpawn>> Rig<X> {
    /// The questions put to the person on the desktop's sheet.
    pub fn sheets(&self) -> Vec<ConfirmRequest> {
        self.router.seams.confirmer.desktop.requests()
    }

    /// Everything the router audited.
    pub fn audit(&self) -> Vec<docket_core::AuditRecord> {
        self.router.seams.sink.records()
    }

    /// How many calls of the pseudo-app's action `action` reached the router.
    pub fn routed(&self, action: &str) -> usize {
        self.audit()
            .iter()
            .filter(|r| matches!(r, docket_core::AuditRecord::Call { action: a, .. } if a.name.as_str() == action))
            .count()
    }

    /// The standing grants docket's store holds.
    pub fn grants(&self) -> Vec<StandingGrant> {
        self.router.standing_grants()
    }
}

/// The world around a backend, built but not started: the real router with the host's performer
/// answering for the pseudo-app, the court the host calls it through, and a fake agent playing
/// `setup.turns`.
pub struct Wired<X: Seams<Court = TheCourt, Sandbox = FakeSandbox, Spawn = FakeSpawn>> {
    pub backend: AcpBackend<X>,
    pub router: Arc<World>,
    pub desk: EditorDesk,
    pub sandbox: Seen,
    pub spawned: SpawnSeen,
    pub agent: View,
    pub performer: Performer<X::Files, FakeSandbox>,
    pub court: TheCourt,
    pub fallback: Fallback,
}

pub fn wired_over<X>(files: X::Files, setup: Setup) -> Wired<X>
where
    X: Seams<Court = TheCourt, Sandbox = FakeSandbox, Spawn = FakeSpawn>,
    X::Files: Clone + 'static,
{
    let desk = EditorDesk::new();
    let clock = FixedClock::at(UnixSeconds(1_760_000_000));
    let confirmer = DeskConfirmer {
        desktop: ScriptedConfirmer::answering(setup.answers),
        host: SheetConfirmer::new(desk.clone(), clock.clone()),
    };
    let writer = match setup.policy {
        Policy::Wide => ScriptedWriter::returning(Ok(wide_policy())),
        Policy::None => ScriptedWriter::failing(),
    };
    let router = fake_router_over(
        setup.config,
        confirmer,
        setup.reviewer,
        writer,
        clock,
        Arc::new(MemoryLog::new()),
    )
    .expect("router");
    for grant in setup.held {
        router.seams.grants.add_standing(grant);
    }
    let (sandbox, seen) = FakeSandbox::ready(setup.scripts);
    let performer = Performer::with_network(files, sandbox, setup.network)
        .with_agent_network(setup.agent_network);
    router.seams.link.host(
        acp_agent_app().expect("app"),
        Arc::new(Hosted(performer.clone())),
    );
    let router = Arc::new(router);
    let court = IntentsCourt::over(InProcess::new(router.clone(), host_caller()));
    let (wire, view) = agent(setup.turns);
    let (spawn, spawned) = FakeSpawn::new(vec![wire]);
    let backend = AcpBackend::<X>::new(Parts {
        program: program(),
        session: SessionId::parse("s-1").expect("session"),
        spawn,
        performer: performer.clone(),
        court: court.clone(),
    });
    Wired {
        backend,
        router,
        desk,
        sandbox: seen,
        spawned,
        agent: view,
        performer,
        court,
        fallback: setup.fallback,
    }
}

/// A started host over `files`: the handshake is done and it waits for a turn.
pub async fn started_over<X>(files: X::Files, cwd: &str, setup: Setup) -> Rig<X>
where
    X: Seams<Court = TheCourt, Sandbox = FakeSandbox, Spawn = FakeSpawn>,
    X::Files: Clone + 'static,
{
    let wired = wired_over::<X>(files, setup);
    let mut host = AgentHost::new(
        wired.backend,
        wired.court.clone(),
        wired.desk.clone(),
        wired.fallback,
    );
    let session = host.open(opening(cwd)).await.expect("open");
    Rig {
        host,
        router: wired.router,
        session,
        desk: wired.desk,
        sandbox: wired.sandbox,
        spawned: wired.spawned,
        agent: wired.agent,
        performer: wired.performer,
        court: wired.court,
    }
}

/// A started host over a file system in a map.
pub async fn started(setup: Setup) -> (Rig<Fakes>, FakeFiles) {
    let files = FakeFiles::new();
    let rig = started_over::<Fakes>(files.clone(), CWD, setup).await;
    (rig, files)
}

/// Gives the host a turn and pulls events to the end of it. A sheet the host is handed (the
/// development fallback) is answered `Refused`.
pub async fn run_turn<X>(rig: &mut Rig<X>, text: &str) -> Vec<BackendEvent>
where
    X: Seams<Court = TheCourt, Sandbox = FakeSandbox, Spawn = FakeSpawn>,
{
    run_turn_answering(rig, 1, text, &mut |_| SheetChoice::Refused).await
}

/// [`run_turn`] numbering the turn `n` and answering the sheets the host hands out with `choose`.
pub async fn run_turn_answering<X>(
    rig: &mut Rig<X>,
    n: u64,
    text: &str,
    choose: &mut dyn FnMut(&ConfirmRequest) -> SheetChoice,
) -> Vec<BackendEvent>
where
    X: Seams<Court = TheCourt, Sandbox = FakeSandbox, Spawn = FakeSpawn>,
{
    let session = rig.session.clone();
    rig.host.turn(&session, turn(n, text)).await.expect("turn");
    let mut events = Vec::new();
    while let Some(event) = rig.host.next_event(&session).await.expect("event") {
        if let BackendEvent::Sheet(request) = &event {
            let choice = choose(request);
            rig.host
                .answer_sheet(&session, &request.id, choice)
                .await
                .expect("answer");
        }
        let over = matches!(event, BackendEvent::TurnEnd(_));
        events.push(event);
        if over {
            break;
        }
    }
    events
}

/// How the last turn ended.
pub fn ended_with(events: &[BackendEvent]) -> &TurnEnd {
    match events.last() {
        Some(BackendEvent::TurnEnd(end)) => end,
        other => panic!("the turn must end: {other:?}"),
    }
}

pub use super::calls::{read, selected, tool, write};
pub use super::world::{DeskConfirmer, Fakes, Hosted, Real, TheCourt, World};
