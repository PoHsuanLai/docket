//! `docket-agent <program> [--cwd DIR]`: runs one configured coding agent (Claude Code through
//! its ACP adapter, say) as a docket session backend, with a prompt on the terminal. For the
//! owner's live check, not a product surface. Off unless `agent.acp.agents` is on in the settings
//! file; the programs come from `agents.toml` under the configuration directory.
//!
//! Every file write, command and permission request the agent makes is put to you here, as the
//! sheet would put it: `y` once, `a` always (when offered; stored as a standing grant for this
//! program, in the data directory), anything else no. What the agent says of itself is printed
//! as the agent's words. Exit codes: 2 for a bad command line, a setting that is off or a
//! configuration that is wrong; 1 for anything else.

use docket_acp::client::{AcpBackend, AgentAsk, Ask, OsFiles, Parts, Seams, What};
use docket_acp::{Answer, SystemTicks, always_words};
use docket_core::{
    AbsPath, AlwaysOffer, StandingGrant, StepEnd, TurnId, TurnSource, TurnVia, UserTurn,
    decode_standing, encode_standing,
};
use docket_launch::dbus::DbusAccounts;
use docket_launch::login::VisibleLogin;
use docket_launch::{
    AgentSpawn, AgentsFile, AgentsPermit, BwrapProcs, ChildProc, Registry, Supervisor,
};
use docket_session::{
    BackendEvent, BackendKind, CallEvent, Opening, ProgramName, SessionBackend, StartSession,
    Workspace,
};
use docket_settings::{AgentSettings, Locator};
use docket_shell::Detected;
use prov::{AgentRef, SessionId, SpaceId, TaskId, UnixSeconds};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader, Stdin};
use tokio::sync::Mutex;

type Lines = Arc<Mutex<tokio::io::Lines<BufReader<Stdin>>>>;

/// Asks on the terminal.
#[derive(Clone)]
struct TtyAsk {
    lines: Lines,
}

impl Ask for TtyAsk {
    async fn ask(&mut self, ask: &AgentAsk) -> Answer {
        let what = match &ask.what {
            What::Write(path) => format!("write {}", path.as_str()),
            What::Command { line, cwd } => format!("run `{line}` in {}", cwd.as_str()),
            What::Tool { kind, paths } => format!(
                "use its {kind} tool on {}",
                paths
                    .iter()
                    .map(AbsPath::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        eprintln!("\n? The agent wants to {what}");
        eprintln!("  (its own words: {})", ask.title.as_str());
        let always = match &ask.offer {
            AlwaysOffer::Offered(scope) => {
                eprintln!("  [a] {}", always_words("this", scope));
                true
            }
            AlwaysOffer::Withheld(_) => false,
        };
        eprint!("  [y] once, [a] always, anything else no: ");
        let answer = self.lines.lock().await.next_line().await.ok().flatten();
        match answer.as_deref().map(str::trim) {
            Some("y") => Answer::Once,
            Some("a") if always => Answer::Always,
            Some("a") => Answer::Once,
            _ => Answer::No,
        }
    }
}

struct Live;

impl Seams for Live {
    type Spawn = AgentSpawn<DbusAccounts, BwrapProcs>;
    type Files = OsFiles;
    type Ask = TtyAsk;
    type Sandbox = Detected;
    type Ticks = SystemTicks;
}

struct Args {
    program: ProgramName,
    cwd: PathBuf,
}

fn args() -> Option<Args> {
    let mut it = std::env::args().skip(1);
    let program = ProgramName::parse(&it.next()?).ok()?;
    let mut cwd = std::env::current_dir().ok()?;
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--cwd" => cwd = PathBuf::from(it.next()?),
            _ => return None,
        }
    }
    Some(Args { program, cwd })
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

fn print(event: &BackendEvent) {
    match event {
        BackendEvent::Words(docket_core::Reveal::Plain(text)) => print!("{text}"),
        BackendEvent::Call(CallEvent::Started(open)) => {
            eprintln!("\n> {} ({:?})", open.action.name, open.effect);
        }
        BackendEvent::Call(CallEvent::Ended(step)) => {
            let how = match &step.end {
                StepEnd::Done { .. } => "done",
                StepEnd::Unconfirmed(_) => "not confirmed",
                StepEnd::Interrupted => "interrupted",
                _ => "refused",
            };
            eprintln!("  = {how}");
        }
        BackendEvent::TurnEnd(end) => eprintln!("\n[turn ended: {end:?}]"),
        _ => {}
    }
}

async fn run(args: Args) -> Result<(), String> {
    let loaded = Locator::from_env(&env).read(AgentSettings::default());
    let permit = AgentsPermit::from_setting(loaded.value.agents).map_err(|e| e.to_string())?;
    let text = std::fs::read_to_string(config_dir().join("docket/agents.toml"))
        .map_err(|_| "no agents.toml under the configuration directory".to_owned())?;
    let file = Arc::new(AgentsFile::parse(&text).map_err(|e| e.to_string())?);
    let path = env("PATH").unwrap_or_default();
    let Detected::Bwrap(bwrap) = Detected::probe(&path) else {
        return Err("no sandbox (bubblewrap) here: no agent is started unconfined".to_owned());
    };
    let forwarder = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("docket-net-forward")))
        .and_then(|p| p.to_str().and_then(|t| AbsPath::parse(t).ok()))
        .ok_or("cannot find docket-net-forward beside this program")?;
    let run_dir = PathBuf::from(env("XDG_RUNTIME_DIR").ok_or("no XDG_RUNTIME_DIR")?);

    let bus = zbus::Connection::session()
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

    let spawn = AgentSpawn::new(
        permit,
        file,
        accounts,
        BwrapProcs::new(bwrap.program().to_owned()),
        run_dir,
        forwarder,
        registry,
    );
    let grants_path = data_dir().join("acp-agent-grants.json");
    let grants: Vec<StandingGrant> = std::fs::read_to_string(&grants_path)
        .ok()
        .and_then(|t| decode_standing(&t).ok())
        .unwrap_or_default();
    let lines: Lines = Arc::new(Mutex::new(BufReader::new(tokio::io::stdin()).lines()));
    let session = SessionId::parse("agent-1").map_err(|e| e.to_string())?;
    let cwd = std::fs::canonicalize(&args.cwd).map_err(|e| e.to_string())?;
    let cwd = cwd.to_str().ok_or("the directory is not UTF-8")?.to_owned();
    let mut backend = AcpBackend::<Live>::new(Parts {
        program: args.program.clone(),
        session: session.clone(),
        spawn,
        files: OsFiles,
        ask: TtyAsk {
            lines: lines.clone(),
        },
        sandbox: Detected::Bwrap(bwrap),
        ticks: SystemTicks,
        grants,
    });
    let opening = Opening {
        task: TaskId::parse("agent-task-1").map_err(|e| e.to_string())?,
        space: SpaceId::desktop(),
        opener: None,
        agent: Some(AgentRef::Companion),
        backend: BackendKind::Acp(args.program),
        parent: None,
        forked_from: None,
        cwd: Some(Workspace::parse(&cwd).map_err(|e| e.to_string())?),
    };
    backend
        .start(StartSession { session, opening })
        .await
        .map_err(|e| e.to_string())?;
    eprintln!("agent started in {cwd}. Type a request; an empty line or ctrl-d quits.");
    let mut n = 0;
    loop {
        eprint!("\n> ");
        let line = lines.lock().await.next_line().await.ok().flatten();
        let Some(text) = line.filter(|l| !l.trim().is_empty()) else {
            break;
        };
        n += 1;
        let turn = UserTurn {
            id: TurnId(n),
            text,
            at: UnixSeconds(0),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        };
        backend.turn(turn).await.map_err(|e| e.to_string())?;
        while let Some(event) = backend.next_event().await {
            print(&event);
        }
    }
    backend.close().await;
    let _ = std::fs::create_dir_all(data_dir());
    let _ = std::fs::write(&grants_path, encode_standing(&backend.grants()));
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let Some(args) = args() else {
        eprintln!("usage: docket-agent PROGRAM [--cwd DIR]");
        return ExitCode::from(2);
    };
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("docket-agent: {why}");
            ExitCode::from(2)
        }
    }
}
