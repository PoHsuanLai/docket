//! Hosting one agent session over any account seam: the performer the router's `Perform` for the
//! pseudo-app reaches, the pseudo-app served on the bus under `org.quire.AcpAgent`, the spawner,
//! the backend, and the session opened at the router. `docket-agent` hosts with porter's
//! accountd behind it; the live-eval harness hosts with `LoginOnly` on its private bus. Nothing
//! here reads the environment or a file: everything is handed in.

use crate::agent::live::Live;
use crate::agent::remember::remember;
use agents_core::dirs::AgentsDir;
use bulkhead::{Detected, NetworkMode};
use docket_acp::client::{AgentHost, EdgeMenu, Fallback, IntentsCourt, OsFiles, Seams, ToolsOffer};
use docket_client::DbusTransport;
use docket_core::AbsPath;
use docket_dbus::BusConnection;
use docket_dbus::callers::Identity;
use docket_inapp::EditorDesk;
use docket_launch::{
    Accounts, AgentSpawn, AgentsFile, AgentsPermit, BwrapProcs, ChildProc, Registry,
};
use docket_session::{
    BackendFault, BackendKind, HostFault, Opening, ProgramName, SessionHost, Workspace,
};
use prov::{AgentRef, SessionId, SpaceId, TaskId};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;

/// The seams of a live host over the account seam `A`.
#[derive(Debug)]
pub struct Hosting<A>(PhantomData<A>);

impl<A: Accounts + 'static> Seams for Hosting<A> {
    type Spawn = AgentSpawn<A, BwrapProcs>;
    type Files = OsFiles;
    type Court = IntentsCourt<DbusTransport>;
    type Sandbox = Detected;
}

/// The host of one agent session.
pub type Host<A> = AgentHost<Hosting<A>, EditorDesk>;

/// Everything a host is built from.
#[allow(missing_debug_implementations)]
pub struct Wiring<A: Accounts + 'static> {
    /// The bus connection that plays `org.quire.AcpAgent`: the host claims the name on it.
    pub bus: BusConnection,
    /// The account seam.
    pub accounts: Arc<A>,
    /// The programs that may start.
    pub file: Arc<AgentsFile>,
    /// The proof that `agent.acp.agents` is on.
    pub permit: AgentsPermit,
    /// The sandbox program found by `Detected::probe`.
    pub bwrap: Detected,
    /// `docket-net-forward`, for an `endpoint_only` or `allowlist` entry.
    pub forwarder: AbsPath,
    /// `docket-net-enter`, the exec stage that limits what an agent does with its files and
    /// network. None when it is not there: the agent is then held by the sandbox alone.
    pub enter: Option<AbsPath>,
    /// A directory only the user can enter, for sockets.
    pub run_dir: PathBuf,
    /// What a revocation looks up: shared with the supervisor that ends a revoked process.
    pub registry: Registry<ChildProc>,
    /// The tool edge to offer the agent, when the host has one to give.
    pub tools: Option<ToolsOffer>,
    /// Who answers sheets the router routes to the host.
    pub fallback: Fallback,
    /// The desk those sheets land on, served as `Confirm1` by the caller when it is used.
    pub desk: EditorDesk,
    /// Who is intentd on the bus: the only process that may ask or withdraw a sheet the host
    /// serves as `Confirm1`.
    pub callers: Arc<Identity>,
    /// The agents directory a registry agent was installed in: what an opening shows of the
    /// agent is written down there. None writes nothing.
    pub agents: Option<AgentsDir>,
}

/// A running host and the session it opened.
#[allow(missing_debug_implementations)]
pub struct Hosted<A: Accounts + 'static> {
    /// The host.
    pub host: Host<A>,
    /// The agent's session at the router.
    pub session: SessionId,
}

/// Why a host did not start: something about this computer, or the session did not open.
#[derive(Debug)]
pub enum HostError {
    /// The sandbox, the manifest or the bus: the words say what.
    Setup(String),
    /// The session did not open; the fault says why (a sign-in is needed, say).
    Open(HostFault),
}

impl From<String> for HostError {
    fn from(words: String) -> Self {
        HostError::Setup(words)
    }
}

impl From<&str> for HostError {
    fn from(words: &str) -> Self {
        HostError::Setup(words.to_owned())
    }
}

impl HostError {
    /// The words a person reads for it.
    pub fn said(self) -> String {
        match self {
            HostError::Setup(words) => words,
            HostError::Open(fault) => said(fault),
        }
    }
}

/// Serves the pseudo-app on the bus, starts the program named `program`, and opens its session in
/// `space` working in `cwd`.
pub async fn host<A: Accounts + 'static>(
    wiring: Wiring<A>,
    program: ProgramName,
    cwd: &str,
    space: SpaceId,
) -> Result<Hosted<A>, String> {
    host_typed(wiring, program, cwd, space)
        .await
        .map_err(HostError::said)
}

/// [`host`], saying why it failed as a [`HostError`] so a caller can tell a missing sign-in from
/// a broken setup.
pub async fn host_typed<A: Accounts + 'static>(
    wiring: Wiring<A>,
    program: ProgramName,
    cwd: &str,
    space: SpaceId,
) -> Result<Hosted<A>, HostError> {
    // The agent process's own network counts for the commands it runs (R12). A program the file
    // does not list cannot be started at all; until then assume the widest.
    let agent_network = wiring
        .file
        .get(&program)
        .map_or(NetworkMode::Host, |entry| entry.network);
    let live = Live::serve(wiring, agent_network).await?;
    open_on(&live, program, cwd, space).await
}

/// Starts the program named `program` on a host already serving the pseudo-app, and opens its
/// session in `space` working in `cwd`. Any number of sessions share one `live`.
pub async fn open_on<A: Accounts + 'static>(
    live: &Live<A>,
    program: ProgramName,
    cwd: &str,
    space: SpaceId,
) -> Result<Hosted<A>, HostError> {
    // Who keeps the agent's file history is the person's choice in `agents.toml`; a program the
    // file does not list gets docket's own (it over-saves rather than under-saves).
    let (label, rewind, installed_as) = {
        let entry = live.entry(&program);
        (
            entry.and_then(|entry| entry.label.clone()),
            entry.map(|entry| entry.rewind).unwrap_or_default(),
            entry.and_then(|entry| entry.registry.clone()),
        )
    };
    let backend = live.backend(&program, EdgeMenu::Everything);
    let mut host = AgentHost::new(
        backend,
        live.court.clone(),
        live.desk.clone(),
        live.fallback,
    )
    .with_rewind(rewind);
    let opened = host
        .open(Opening {
            task: TaskId::parse("agent-task-1").map_err(|e| e.to_string())?,
            space,
            opener: None,
            agent: Some(AgentRef::Companion),
            backend: BackendKind::Acp(program),
            parent: None,
            forked_from: None,
            started_from: None,
            label,
            cwd: Some(Workspace::parse(cwd).map_err(|e| e.to_string())?),
            rewind: Default::default(),
        })
        .await;
    if let (Some(dir), Some(id)) = (&live.agents, &installed_as) {
        remember(dir, id, host.backend(), opened.as_ref().map(|_| ()));
    }
    let session = opened.map_err(HostError::Open)?;
    Ok(Hosted { host, session })
}

/// Why the session did not open, in words a person reads: a sign-in problem is said plainly,
/// not as "the backend failed".
fn said(fault: HostFault) -> String {
    match fault {
        HostFault::Backend(
            fault @ (BackendFault::SignInNeeded
            | BackendFault::SignInUnsupported
            | BackendFault::SignInChoose(_)
            | BackendFault::ModelNotOffered(_)
            | BackendFault::ModelNotChoosable
            | BackendFault::ModelRefused),
        ) => fault.to_string(),
        other => other.to_string(),
    }
}
