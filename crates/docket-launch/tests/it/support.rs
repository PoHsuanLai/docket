//! Fixtures: the agents.toml of each route, and a launcher wired to the fakes.
#![allow(dead_code)]

use docket_acp::client::LaunchPlan;
use docket_acp::client::fake::{ChannelWire, pipe};
use docket_core::AbsPath;
use docket_launch::fake::{FakeAccounts, FakeProc, FakeProcs, Mood, ProcsSeen, Secrets};
use docket_launch::{AgentSpawn, AgentsFile, AgentsPermit, Registry};
use docket_session::ProgramName;
use prov::SessionId;
use std::sync::Arc;

pub const CWD: &str = "/work/app";

pub fn abs(text: &str) -> AbsPath {
    AbsPath::parse(text).expect("abs")
}

/// Claude Code through inferd: a base URL and a per-session token, network to the endpoint only.
pub const ENDPOINT: &str = r#"
[[agent]]
program = "claude-code"
command = "/home/me/.local/bin/claude-agent-acp"
args = ["--stdio"]
route = "endpoint"
network = "endpoint_only"
key_env = "ANTHROPIC_API_KEY"
base_url_env = "ANTHROPIC_BASE_URL"
protocol = "anthropic_messages"
reads = ["/home/me/.local/share/node"]
state = ["/home/me/.claude", "/home/me/.claude.json"]
home = "/home/me"
[agent.endpoint]
kind = "account"
id = "anthropic-main"
[agent.set]
NODE_NO_WARNINGS = "1"
"#;

/// An agent that takes a key and cannot take a base URL: a handoff, delivered as a value.
pub const HANDOFF: &str = r#"
[[agent]]
program = "gemini-cli"
command = "/opt/gemini/bin/gemini-acp"
route = "handoff"
network = "host"
key_env = "GEMINI_API_KEY"
protocol = "generate_content"
"#;

pub const HANDOFF_FILE: &str = r#"
[[agent]]
program = "gemini-cli"
command = "/opt/gemini/bin/gemini-acp"
route = "handoff"
network = "host"
key_env = "GEMINI_API_KEY"
delivery = "file"
"#;

/// A subscription login: the agent's own sign-in, nothing from us, the host network.
pub const LOGIN: &str = r#"
[[agent]]
program = "claude-code"
command = "/home/me/.local/bin/claude-agent-acp"
route = "login"
network = "host"
state = ["/home/me/.claude"]
home = "/home/me"
login = ["konsole", "-e", "claude", "login"]
"#;

pub fn permit() -> AgentsPermit {
    AgentsPermit::from_text("[agent.acp]\nagents = \"on\"\n").expect("on")
}

pub fn plan(program: &str, session: &str) -> LaunchPlan {
    LaunchPlan {
        program: ProgramName::parse(program).expect("program"),
        session: SessionId::parse(session).expect("session"),
        cwd: abs(CWD),
        edge: None,
    }
}

pub struct Rig {
    pub spawn: AgentSpawn<FakeAccounts, FakeProcs>,
    pub accounts: FakeAccounts,
    pub procs: ProcsSeen,
    pub registry: Registry<FakeProc>,
    pub file: Arc<AgentsFile>,
    pub secrets: Secrets,
    pub dir: tempfile::TempDir,
    /// The agent's end of the pipe, for tests that talk to it.
    pub agent_end: Option<ChannelWire>,
}

pub fn rig(toml: &str, mood: Mood, with_process: bool) -> Rig {
    let secrets = Secrets::default();
    let accounts = FakeAccounts::with(mood, secrets.clone());
    let (near, far) = pipe();
    let wires = if with_process { vec![near] } else { Vec::new() };
    let (procs, seen) = FakeProcs::new(wires);
    let dir = tempfile::tempdir().expect("scratch");
    let file = Arc::new(AgentsFile::parse(toml).expect("agents.toml"));
    let registry = Registry::default();
    let spawn = AgentSpawn::new(
        permit(),
        file.clone(),
        Arc::new(accounts.clone()),
        procs,
        dir.path().to_owned(),
        abs("/opt/docket/docket-net-forward"),
        registry.clone(),
    );
    Rig {
        spawn,
        accounts,
        procs: seen,
        registry,
        file,
        secrets,
        dir,
        agent_end: Some(far),
    }
}

/// A subscription login with the Claude Code preset; `{state}` is where its state lives.
pub fn confined(state: &str, set: &str) -> String {
    format!(
        r#"
[[agent]]
program = "claude-code"
command = "/home/me/.local/bin/claude-agent-acp"
route = "login"
network = "host"
state = ["{state}"]
home = "/home/me"
profile = "claude-code"
[agent.set]
{set}
"#
    )
}
