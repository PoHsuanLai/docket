//! Hosting one agent session over any account seam: the performer the router's `Perform` for the
//! pseudo-app reaches, the pseudo-app served on the bus under `org.quire.AcpAgent`, the spawner,
//! the backend, and the session opened at the router. `docket-agent` hosts with porter's
//! accountd behind it; the live-eval harness hosts with `LoginOnly` on its private bus. Nothing
//! here reads the environment or a file: everything is handed in.

use crate::Wall;
use crate::agent::provider::{PerformerProvider, Quiet};
use docket_acp::client::{
    AcpBackend, AgentHost, Fallback, IntentsCourt, OsFiles, Parts, Performer, Seams, ToolsOffer,
};
use docket_client::{DbusTransport, serve_on};
use docket_core::{ACP_AGENT_APP, AbsPath, ValidManifest};
use docket_dbus::{BusConnection, CONFIRM_PATH};
use docket_inapp::EditorDesk;
use docket_launch::{
    Accounts, AgentSpawn, AgentsFile, AgentsPermit, BwrapProcs, ChildProc, Registry,
};
use docket_session::{BackendKind, Opening, ProgramName, SessionHost, Workspace};
use docket_shell::{Detected, NetworkMode};
use prov::{AgentRef, SessionId, SpaceId, TaskId};
use std::marker::PhantomData;
use std::path::PathBuf;
use std::sync::Arc;

const MANIFEST: &str = include_str!("../../../../manifests/org.quire.AcpAgent.toml");

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
    /// `docket-net-forward`, for an `endpoint_only` entry.
    pub forwarder: AbsPath,
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
}

/// A running host and the session it opened.
#[allow(missing_debug_implementations)]
pub struct Hosted<A: Accounts + 'static> {
    /// The host.
    pub host: Host<A>,
    /// The agent's session at the router.
    pub session: SessionId,
}

/// Serves the pseudo-app on the bus, starts the program named `program`, and opens its session in
/// `space` working in `cwd`.
pub async fn host<A: Accounts + 'static>(
    wiring: Wiring<A>,
    program: ProgramName,
    cwd: &str,
    space: SpaceId,
) -> Result<Hosted<A>, String> {
    let Wiring {
        bus,
        accounts,
        file,
        permit,
        bwrap,
        forwarder,
        run_dir,
        registry,
        tools,
        fallback,
        desk,
    } = wiring;
    let Detected::Bwrap(found) = &bwrap else {
        return Err("no sandbox (bubblewrap) here: no agent is started unconfined".to_owned());
    };
    let program_path = found.program().to_owned();
    // The agent process's own network counts for the commands it runs (R12). A program the file
    // does not list cannot be started at all; until then assume the widest.
    let agent_network = file
        .get(&program)
        .map_or(NetworkMode::Host, |entry| entry.network);
    let performer = Performer::new(OsFiles, bwrap).with_agent_network(agent_network);
    let manifest: ValidManifest =
        docket_router::parse(MANIFEST).map_err(|e| format!("the agent manifest: {e}"))?;
    let quiet = Quiet(docket_core::acp_agent_app().ok_or("no app name for the agent host")?);
    if fallback == Fallback::Terminal {
        // Served before the name is claimed, so the first sheet finds it.
        bus.object_server()
            .at(
                CONFIRM_PATH,
                crate::confirm::ConfirmObject::new(desk.clone(), Wall),
            )
            .await
            .map_err(|e| e.to_string())?;
    }
    serve_on(
        &bus,
        PerformerProvider::new(manifest, performer.clone()),
        quiet.clone(),
        quiet,
    )
    .await
    .map_err(|e| format!("{ACP_AGENT_APP}: {e}"))?;

    let spawn = AgentSpawn::new(
        permit,
        file,
        accounts,
        BwrapProcs::new(program_path),
        run_dir,
        forwarder,
        registry,
    );
    let court = IntentsCourt::over(DbusTransport::new(bus));
    let backend = AcpBackend::<Hosting<A>>::new(Parts {
        program: program.clone(),
        session: SessionId::parse("s-0").map_err(|e| e.to_string())?,
        spawn,
        performer,
        court: court.clone(),
        tools,
    });
    let mut host = AgentHost::new(backend, court, desk, fallback);
    let session = host
        .open(Opening {
            task: TaskId::parse("agent-task-1").map_err(|e| e.to_string())?,
            space,
            opener: None,
            agent: Some(AgentRef::Companion),
            backend: BackendKind::Acp(program),
            parent: None,
            forked_from: None,
            started_from: None,
            cwd: Some(Workspace::parse(cwd).map_err(|e| e.to_string())?),
        })
        .await
        .map_err(|e| e.to_string())?;
    Ok(Hosted { host, session })
}
