//! One turn of the person's words on a hosted external agent, in a running world.
//!
//! The host is the one `docket-agent` runs (`docket_acp_bin::agent::host`), with no accountd
//! behind it (`LoginOnly`): the agent's sandbox is bubblewrap, its calls are router calls over the
//! world's private bus, the sheets go to the world's scripted person (intentd's confirmer), and
//! the per-session tool edge is offered with the bridge program of `Binaries::actions_mcp`.

use super::spec::AcpSpec;
use crate::live::acp::secret::Credentials;
use crate::runlink::short_run;
use crate::world::{Binaries, World};
use bulkhead::Detected;
use docket_acp::client::{Fallback, Models, ToolsOffer};
use docket_acp_bin::agent::host::{Hosted, Wiring, host};
use docket_core::{AbsPath, TurnId, TurnSource, TurnVia, UserTurn};
use docket_inapp::EditorDesk;
use docket_launch::{AgentsFile, AgentsPermit, LoginOnly, Registry};
use docket_session::{BackendEvent, EndCause, ProgramName, SessionHost, TurnEnd};
use prov::{SpaceId, UnixSeconds};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

/// How an agent's turn came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEnd {
    /// The host reported this end.
    Turn(TurnEnd),
    /// The agent said nothing more for as long as the run was willing to wait.
    Silent,
    /// The agent never started, and why (coarse, never a path or a secret).
    NotStarted(String),
}

/// What a turn showed.
#[derive(Debug, Clone)]
pub struct Played {
    /// How it ended.
    pub end: AgentEnd,
    /// What the agent said, in order.
    pub words: Vec<String>,
    /// The calls the host announced and ended, one line each.
    pub calls: Vec<String>,
}

impl Played {
    fn failed(why: impl Into<String>) -> Self {
        Self {
            end: AgentEnd::NotStarted(why.into()),
            words: Vec::new(),
            calls: Vec::new(),
        }
    }
}

fn abs(path: &Path) -> Option<AbsPath> {
    path.to_str().and_then(|t| AbsPath::parse(t).ok())
}

/// The directories and files the agent keeps its state in exist before it starts: a bind of a
/// missing path would fail the sandbox.
fn prepare_state(spec: &AcpSpec, home: &Path) -> std::io::Result<()> {
    for entry in &spec.state {
        let path = home.join(entry);
        if path.extension().is_some_and(|e| e == "json") {
            if !path.exists() {
                std::fs::write(&path, "{}")?;
            }
        } else {
            std::fs::create_dir_all(&path)?;
        }
    }
    Ok(())
}

/// Hosts `spec` in `world` the way `docket-agent` does and opens its session; the scratch
/// directories exist first. The reason, when it does not open, is said plainly.
async fn open(
    world: &World,
    binaries: &Binaries,
    spec: &AcpSpec,
) -> Result<Hosted<LoginOnly>, String> {
    let home = world.dir.path();
    let work = home.join("work");
    std::fs::create_dir_all(&work)
        .and_then(|()| prepare_state(spec, home))
        .map_err(|why| format!("scratch directories: {why}"))?;
    let (Some(forwarder), Some(cwd)) = (abs(&binaries.actions_mcp), abs(&work)) else {
        return Err("a path is not UTF-8".to_owned());
    };
    let entry = spec.entry_toml(home);
    let dir = spec
        .registry
        .as_ref()
        .map(|pick| docket_agents::AgentsDir::at(&pick.dir));
    let file = AgentsFile::parse_in(&entry, dir.as_ref()).map_err(|why| why.to_string())?;
    let permit = AgentsPermit::from_text("[agent.acp]\nagents = \"on\"\n")
        .map_err(|_| "agents are off".to_owned())?;
    let program = ProgramName::parse(&spec.program).map_err(|_| "the program name".to_owned())?;
    let wiring = Wiring {
        bus: world.connect().await,
        accounts: Arc::new(LoginOnly),
        file: Arc::new(file),
        permit,
        bwrap: Detected::probe("/usr/bin:/bin:/usr/local/bin"),
        forwarder,
        run_dir: short_run(home),
        registry: Registry::default(),
        tools: abs(&binaries.actions_mcp).map(|bridge| ToolsOffer {
            run_dir: short_run(home),
            bridge,
        }),
        fallback: Fallback::Off,
        desk: EditorDesk::new(),
        agents: dir,
    };
    let space = SpaceId::parse("work").unwrap_or_else(|_| SpaceId::desktop());
    host(wiring, program, cwd.as_str(), space).await
}

/// The models the agent offers for a session opened in `world`, and the one in use; none when it
/// offers no choice. The session is closed again without a turn.
pub async fn offered_models(
    world: &World,
    binaries: &Binaries,
    spec: &AcpSpec,
) -> Result<Option<Models>, String> {
    let Hosted { mut host, session } = open(world, binaries, spec).await?;
    let models = host.backend().models().cloned();
    let _ = host.close(&session, EndCause::Closed).await;
    Ok(models)
}

/// Hosts `spec` in `world`, gives it `prompt`, and watches the turn to its end, for at most
/// `patience` between events. `credentials` is the staged login, kept alive for the turn.
pub async fn play(
    world: &World,
    binaries: &Binaries,
    spec: &AcpSpec,
    _credentials: Option<&Credentials>,
    prompt: &str,
    patience: Duration,
) -> Played {
    let Hosted { mut host, session } = match open(world, binaries, spec).await {
        Ok(hosted) => hosted,
        Err(why) => return Played::failed(why),
    };
    let turn = UserTurn {
        id: TurnId(1),
        text: prompt.to_owned(),
        at: UnixSeconds(1),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    };
    let mut played = Played {
        end: AgentEnd::Silent,
        words: Vec::new(),
        calls: Vec::new(),
    };
    if host.turn(&session, turn).await.is_err() {
        played.end = AgentEnd::NotStarted("the host refused the turn".to_owned());
    }
    while matches!(played.end, AgentEnd::Silent) {
        let next = tokio::time::timeout(patience, host.next_event(&session)).await;
        match next {
            Err(_) | Ok(Ok(None) | Err(_)) => break,
            Ok(Ok(Some(event))) => note(&mut played, event),
        }
    }
    let _ = host.close(&session, EndCause::Closed).await;
    played
}

fn note(played: &mut Played, event: BackendEvent) {
    match event {
        BackendEvent::Words(docket_core::Reveal::Plain(text)) => played.words.push(text),
        BackendEvent::Call(call) => played.calls.push(format!("{call:?}")),
        BackendEvent::TurnEnd(end) => played.end = AgentEnd::Turn(end),
        BackendEvent::Words(_)
        | BackendEvent::Thought(_)
        | BackendEvent::NeedsYou(_)
        | BackendEvent::Sheet(_)
        | BackendEvent::Usage(_) => {}
    }
}
