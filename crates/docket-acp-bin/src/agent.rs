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
pub mod host;
pub mod provider;
pub mod tty;

use args::Args;
use docket_acp::client::ToolsOffer;
use docket_core::AbsPath;
use docket_inapp::EditorDesk;
use docket_launch::dbus::DbusAccounts;
use docket_launch::login::VisibleLogin;
use docket_launch::{AgentsFile, AgentsPermit, ChildProc, Registry, Supervisor, ToolsMode};
use docket_session::{BackendEvent, SessionHost};
use docket_settings::{AgentSettings, Locator};
use docket_shell::Detected;
use host::{Hosted, Wiring, host};
use prov::UnixSeconds;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader, Stdin};
use tokio::sync::Mutex;

type Lines = Arc<Mutex<tokio::io::Lines<BufReader<Stdin>>>>;

/// The seams of the live host: porter's accountd behind it.
pub type Live = host::Hosting<DbusAccounts>;

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

    if data_dir().join("acp-agent-grants.json").exists() {
        eprintln!(
            "docket-agent: ignoring the old acp-agent-grants.json in the data directory: \
             an agent's grants are docket's now (Settings lists and revokes them); allow \
             \"always\" again where you want one"
        );
    }
    let cwd = std::fs::canonicalize(&args.cwd).map_err(|e| e.to_string())?;
    let cwd = cwd.to_str().ok_or("the directory is not UTF-8")?.to_owned();
    let desk = EditorDesk::new();
    let Hosted {
        mut host, session, ..
    } = host(
        Wiring {
            bus,
            accounts,
            file,
            permit,
            bwrap: Detected::Bwrap(bwrap),
            forwarder,
            run_dir,
            registry,
            tools,
            fallback: args.fallback,
            desk,
        },
        args.program,
        &cwd,
        args.space,
    )
    .await?;
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
