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
//! `docket-agent --serve` is the same host as a service for the assistant (see [`serve`]).
//! Exit codes: 2 for a bad command line, a setting that is off or a configuration that is wrong;
//! 1 for anything else.

pub mod args;
pub mod brought_hosts;
pub mod calls;
pub mod fault;
pub mod folders;
pub mod hand_words;
pub mod helper_host;
pub mod helper_hosts;
pub mod host;
pub mod live;
pub mod logout;
pub mod places;
pub mod provider;
pub mod remember;
pub mod serve;
pub mod tty;
pub mod wire;

use args::{Args, Mode};
use docket_acp_host::Workdir;
use docket_core::AbsPath;
use docket_session::{BackendEvent, SessionHost};
use fault::AgentFault;
use host::{Hosted, host};
use places::Places;
use prov::UnixSeconds;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader, Stdin};
use tokio::sync::Mutex;
use wire::{SandboxLimits, Tools, allowed};

type Lines = Arc<Mutex<tokio::io::Lines<BufReader<Stdin>>>>;

/// The program `name` in the directory of this one (it may not be built).
fn sibling(name: &str) -> Option<AbsPath> {
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join(name)))
        .and_then(|p| p.to_str().and_then(|t| AbsPath::parse(t).ok()))
}

/// Runs the host until the person quits. `env` is the environment `main` hands in.
pub async fn run(
    args: Args,
    env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
) -> Result<(), AgentFault> {
    let tools = match args.mode {
        Mode::Session => Tools::Wanted,
        Mode::Refresh => Tools::Unwanted,
    };
    let places = Places::read(env);
    let bus = docket_dbus::session_connection(&|name: &str| places.var(name))
        .await
        .map_err(|e| AgentFault::Bus(e.to_string()))?;
    let wiring = wire::build(
        allowed(&places)?,
        tools,
        args.fallback,
        bus,
        SandboxLimits::Scope,
        places.clone(),
    )
    .await?;

    let cwd = std::fs::canonicalize(&args.cwd).map_err(|e| AgentFault::Directory(e.to_string()))?;
    let cwd = cwd.to_str().ok_or(AgentFault::DirectoryNotUtf8)?.to_owned();
    // The agent gets this folder read-write: never the home folder itself (its dot-directories
    // hold the person's configuration), nor a place of secrets.
    let folder = AbsPath::parse(&cwd).map_err(|e| AgentFault::Directory(e.to_string()))?;
    Workdir::resolve(&folder, &folder, places.home())
        .map_err(|e| AgentFault::Directory(format!("{cwd}: {e}; give --cwd a project folder")))?;
    let Hosted {
        mut host, session, ..
    } = host(wiring, args.program, &cwd, args.space)
        .await
        .map_err(|why| AgentFault::Open(why.to_string()))?;
    if args.mode == Mode::Refresh {
        host.close(&session, docket_session::EndCause::Closed)
            .await
            .map_err(|e| AgentFault::Session(e.to_string()))?;
        return Ok(());
    }
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
            mode: None,
        };
        host.turn(&session, turn)
            .await
            .map_err(|e| AgentFault::Session(e.to_string()))?;
        while let Some(event) = host
            .next_event(&session)
            .await
            .map_err(|e| AgentFault::Session(e.to_string()))?
        {
            if let BackendEvent::Confirm(request) = &event {
                let (words, always) = tty::render(request);
                eprint!("{words}");
                let typed = lines.lock().await.next_line().await.ok().flatten();
                host.answer_sheet(&session, &request.id, tty::choice(typed.as_deref(), always))
                    .await
                    .map_err(|e| AgentFault::Session(e.to_string()))?;
            }
            tty::print(&event);
            if matches!(event, BackendEvent::TurnEnd(_)) {
                break;
            }
        }
    }
    host.close(&session, docket_session::EndCause::Closed)
        .await
        .map_err(|e| AgentFault::Session(e.to_string()))?;
    Ok(())
}
