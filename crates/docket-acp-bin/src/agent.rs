//! `docket-agent <program> [--cwd DIR] [--tty]`: runs one configured coding agent (Claude Code
//! through its ACP adapter, say) as a docket session, with a prompt on the terminal. The host of
//! the agent: it launches the process under the launcher's sandbox, opens the agent's session at
//! the router, records the person's prompts there (the task policy derives from them), and makes
//! every call the agent asks of it as a router call, performing it only after the router allowed
//! it. Off unless `agent.acp.agents` is on in the settings file; the programs come from
//! `agents.toml` under the configuration directory.
//!
//! The person is asked the way every call is: the router's sheet, through intentd's confirmer
//! (sill's). `--tty` is a development fallback: the host asks intentd to hand it this session's
//! sheets and puts them to you on the terminal, `y` once, `a` always (when offered), anything
//! else no. Standing grants live in docket's store, listed and revoked in Settings, not here.
//!
//! The router tells this process by its bus name, `org.quire.AcpAgent`, which `intentd.toml`
//! lists under `acp_agent` (it opens the agent's session, records its turns and makes its calls).
//! Exit codes: 2 for a bad command line, a setting that is off or a configuration that is wrong;
//! 1 for anything else.

pub mod args;
pub mod provider;
pub mod tty;

use crate::Wall;
use crate::confirm::ConfirmObject;
use args::Args;
use docket_acp::client::{
    AcpBackend, AgentHost, Fallback, IntentsCourt, OsFiles, Parts, Performer, Seams, ToolsOffer,
};
use docket_client::{DbusTransport, serve_on};
use docket_core::{ACP_AGENT_APP, AbsPath, ValidManifest};
use docket_dbus::CONFIRM_PATH;
use docket_inapp::EditorDesk;
use docket_launch::dbus::DbusAccounts;
use docket_launch::login::VisibleLogin;
use docket_launch::{
    AgentSpawn, AgentsFile, AgentsPermit, BwrapProcs, ChildProc, Registry, Supervisor, ToolsMode,
};
use docket_session::{BackendEvent, BackendKind, Opening, SessionHost, Workspace};
use docket_settings::{AgentSettings, Locator};
use docket_shell::{Detected, NetworkMode};
use prov::{AgentRef, SessionId, SpaceId, TaskId, UnixSeconds};
use provider::{PerformerProvider, Quiet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader, Stdin};
use tokio::sync::Mutex;

const MANIFEST: &str = include_str!("../../../manifests/org.quire.AcpAgent.toml");

type Lines = Arc<Mutex<tokio::io::Lines<BufReader<Stdin>>>>;

/// The seams of the live host.
#[derive(Debug)]
pub struct Live;

impl Seams for Live {
    type Spawn = AgentSpawn<DbusAccounts, BwrapProcs>;
    type Files = OsFiles;
    type Court = IntentsCourt<DbusTransport>;
    type Sandbox = Detected;
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

fn config_dir() -> PathBuf {
    Locator::from_env(&env)
        .dirs()
        .first()
        .cloned()
        .unwrap_or_default()
}

fn data_dir() -> PathBuf {
    env("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env("HOME").unwrap_or_default()).join(".local/share"))
        .join("docket")
}

/// The program `name` in the directory of this one (it may not be built).
fn sibling(name: &str) -> Option<AbsPath> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(name)))
        .and_then(|p| p.to_str().and_then(|t| AbsPath::parse(t).ok()))
}

/// Runs the host until the person quits.
pub async fn run(args: Args) -> Result<(), String> {
    let loaded = Locator::from_env(&env).read(AgentSettings::default());
    let permit = AgentsPermit::from_setting(loaded.value.agents).map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(config_dir().join("docket/agents.toml"))
        .map_err(|_| "no agents.toml under the configuration directory".to_owned())?;
    let file = Arc::new(AgentsFile::parse(&text).map_err(|e| e.to_string())?);
    let path = env("PATH").unwrap_or_default();
    let Detected::Bwrap(bwrap) = Detected::probe(&path) else {
        return Err("no sandbox (bubblewrap) here: no agent is started unconfined".to_owned());
    };
    let forwarder = sibling("docket-net-forward")
        .ok_or("cannot find docket-net-forward beside this program")?;
    let run_dir = PathBuf::from(env("XDG_RUNTIME_DIR").ok_or("no XDG_RUNTIME_DIR")?);
    // The desktop's actions go to the agent as an MCP server it starts itself: the `actions-mcp`
    // program beside this one, as the bridge to this host. An entry can switch it off.
    let tools = file
        .get(&args.program)
        .filter(|entry| entry.tools == ToolsMode::Offered)
        .and_then(|_| sibling("actions-mcp"))
        .filter(|bridge| std::path::Path::new(bridge.as_str()).exists())
        .map(|bridge| ToolsOffer {
            run_dir: run_dir.clone(),
            bridge,
        });
    if tools.is_none() {
        eprintln!(
            "docket-agent: the agent is not offered the desktop's actions (off, or no actions-mcp beside this program)"
        );
    }

    let bus = docket_dbus::session_connection(&env)
        .await
        .map_err(|e| e.to_string())?;
    let accounts = Arc::new(
        DbusAccounts::connect(&bus)
            .await
            .map_err(|e| e.to_string())?,
    );
    let registry: Registry<ChildProc> = Registry::default();
    let login_env: Vec<(String, String)> = [
        "HOME",
        "PATH",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
    ]
    .iter()
    .filter_map(|k| env(k).map(|v| ((*k).to_owned(), v)))
    .collect();
    let supervisor = Supervisor::new(
        accounts.clone(),
        file.clone(),
        registry.clone(),
        VisibleLogin::new(login_env),
    );
    supervisor.register().await.map_err(|e| e.to_string())?;
    tokio::spawn(async move { supervisor.run().await });

    // The performer is what the router's `Perform` for the pseudo-app reaches, over the bus, and
    // the backend stages its requests with it.
    // The agent process's own network counts for the commands it runs (R12). A program the file
    // does not list cannot be started at all; until then assume the widest.
    let agent_network = file
        .get(&args.program)
        .map_or(NetworkMode::Host, |entry| entry.network);
    let performer =
        Performer::new(OsFiles, Detected::Bwrap(bwrap.clone())).with_agent_network(agent_network);
    let manifest: ValidManifest =
        docket_router::parse(MANIFEST).map_err(|e| format!("the agent manifest: {e}"))?;
    let quiet = Quiet(docket_core::acp_agent_app().ok_or("no app name for the agent host")?);
    let desk = EditorDesk::new();
    if args.fallback == Fallback::Terminal {
        // Served before the name is claimed, so the first sheet finds it.
        bus.object_server()
            .at(CONFIRM_PATH, ConfirmObject::new(desk.clone(), Wall))
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
        BwrapProcs::new(bwrap.program().to_owned()),
        run_dir,
        forwarder,
        registry,
    );
    if data_dir().join("acp-agent-grants.json").exists() {
        eprintln!(
            "docket-agent: ignoring the old acp-agent-grants.json in the data directory: \
             an agent's grants are docket's now (Settings lists and revokes them); allow \
             \"always\" again where you want one"
        );
    }
    let court = IntentsCourt::over(DbusTransport::new(bus));
    let backend = AcpBackend::<Live>::new(Parts {
        program: args.program.clone(),
        session: SessionId::parse("s-0").map_err(|e| e.to_string())?,
        spawn,
        performer,
        court: court.clone(),
        tools,
    });
    let mut host = AgentHost::new(backend, court, desk, args.fallback);

    let cwd = std::fs::canonicalize(&args.cwd).map_err(|e| e.to_string())?;
    let cwd = cwd.to_str().ok_or("the directory is not UTF-8")?.to_owned();
    let session = host
        .open(Opening {
            task: TaskId::parse("agent-task-1").map_err(|e| e.to_string())?,
            space: SpaceId::desktop(),
            opener: None,
            agent: Some(AgentRef::Companion),
            backend: BackendKind::Acp(args.program),
            parent: None,
            forked_from: None,
            started_from: None,
            cwd: Some(Workspace::parse(&cwd).map_err(|e| e.to_string())?),
        })
        .await
        .map_err(|e| e.to_string())?;
    eprintln!("agent started in {cwd}. Type a request; an empty line or ctrl-d quits.");

    let lines: Lines = Arc::new(Mutex::new(BufReader::new(tokio::io::stdin()).lines()));
    let mut n = 0;
    loop {
        eprint!("\n> ");
        let line = lines.lock().await.next_line().await.ok().flatten();
        let Some(text) = line.filter(|l| !l.trim().is_empty()) else {
            break;
        };
        n += 1;
        let turn = docket_core::UserTurn {
            id: docket_core::TurnId(n),
            text,
            at: UnixSeconds(0),
            from: docket_core::TurnSource::Launcher,
            via: docket_core::TurnVia::Typed,
        };
        host.turn(&session, turn).await.map_err(|e| e.to_string())?;
        while let Some(event) = host.next_event(&session).await.map_err(|e| e.to_string())? {
            if let BackendEvent::Sheet(request) = &event {
                let (words, always) = tty::render(request);
                eprint!("{words}");
                let typed = lines.lock().await.next_line().await.ok().flatten();
                host.answer_sheet(&session, &request.id, tty::choice(typed.as_deref(), always))
                    .await
                    .map_err(|e| e.to_string())?;
            }
            tty::print(&event);
            if matches!(event, BackendEvent::TurnEnd(_)) {
                break;
            }
        }
    }
    host.close(&session, docket_session::EndCause::Closed)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
