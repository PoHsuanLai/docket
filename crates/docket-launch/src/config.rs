//! The agent programs the person lists in `agents.toml`: what to run, how it takes a model
//! endpoint or a key, what it may see and write, and how much network it gets. Nothing starts
//! unless `agent.acp.agents` is on and the program is listed here; a prompt cannot add one.
//!
//! ```toml
//! [[agent]]
//! program = "claude-code"
//! command = "/home/me/.local/bin/claude-agent-acp"
//! route = "endpoint"                 # endpoint (inferd, P4) | handoff (a key, P2) | login
//! network = "endpoint_only"          # none | endpoint_only | allowlist | host
//! key_env = "ANTHROPIC_API_KEY"
//! base_url_env = "ANTHROPIC_BASE_URL"
//! protocol = "anthropic_messages"
//! reads = ["/home/me/.local/share/node"]    # read-only: where the program is installed
//! state = ["/home/me/.claude"]              # read-write: its own login and settings
//! home = "/home/me"
//! label = "Claude Code"             # optional: what the audit and the journal call it
//! tools = "offered"                  # offered (default) | off: the desktop's actions as an MCP server
//! profile = "claude-code"            # optional: confine the program's own extras (see `managed`)
//! sign_in = "oauth-personal"         # optional: the way the agent signs itself in at start
//! model = "claude-sonnet-4"          # optional: the model to switch to, one the agent offers
//! checkpoints = "agent"              # optional: docket | agent, who keeps the history of its file changes
//! resume = ["--resume", "{session}"]  # optional: the app's own arguments to pick a conversation up in a terminal
//! [agent.endpoint]
//! kind = "account"
//! id = "anthropic-main"
//! ```
//!
//! An agent that has to reach its provider can be given just the sites it needs instead of the host's
//! network (`network = "allowlist"`): a filter outside its sandbox lets through the named hosts and
//! nothing else, and a deny entry beats an allow entry. `allow` is required; `deny` and `ports` are
//! optional (ports 443 and 80 are always open, `ports` adds more). The lists only go with `allowlist`.
//!
//! ```toml
//! [[agent]]
//! program = "gemini-cli"
//! command = "/opt/gemini/bin/gemini-acp"
//! route = "handoff"
//! network = "allowlist"
//! allow = ["generativelanguage.googleapis.com", "*.googleapis.com"]   # a host, or *.domain
//! deny = ["upload.googleapis.com"]                                    # optional; wins over allow
//! ports = [8443]                                                      # optional; besides 443 and 80
//! key_env = "GEMINI_API_KEY"
//! ```
//!
//! An agent from the agent registry names the registry instead of a command; docket installed it
//! (`docket-agents`) and gives it a home of its own, where it signs in:
//!
//! ```toml
//! [[agent]]
//! program = "antigravity"
//! registry = "antigravity-acp"       # the registry's id for it
//! version = "1.3.0"                  # the pinned version: a newer one is installed only by name
//! route = "login"                    # the default for a registry agent
//! sign_in = "google"                 # optional: which way to sign in, one the agent lists
//! model = "gemini-pro-agent"         # optional: one the agent offers
//! label = "Antigravity"
//! ```

mod check;

use crate::reach::Reach;
use agents_core::dirs::AgentsDir;
use agents_core::launch::LaunchFault;
use bulkhead::NetworkMode;
use bulkhead::egress::HostFault;
use docket_acp_host::{ModelId, SignIn};
use docket_core::{AbsPath, Rewind};
use docket_session::ProgramName;
use porter_core::DataClass;
use porter_core::capability::{AgentProgram, AgentProtocol, EnvName};
use serde::Deserialize;
use std::collections::BTreeMap;

/// How the agent gets a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Route {
    /// inferd opens a per-session loopback endpoint (P4): the agent gets a base URL and a
    /// per-session token, and the real key never leaves porter. The default.
    Endpoint,
    /// accountd hands the agent's process a key (P2), for an agent that cannot take a base URL.
    Handoff,
    /// The agent signs itself in (a subscription, its own key): docket gives it nothing.
    Login,
}

/// How a handed-off key reaches the child.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    /// In the child's environment: for programs that read only their key variable. Readable by
    /// the user's other processes through `/proc`; porter's note on residual risk applies.
    #[default]
    Value,
    /// `<key_env>_FILE` names a 0600 file on a tmpfs, bound read-only into the sandbox: for
    /// programs that read such a variable. The key is in no environment.
    File,
}

/// Whether the agent is offered the desktop's actions over the per-session tool edge
/// (`docket_acp_host::ToolsEdge`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolsMode {
    /// `session/new` offers an MCP server whose calls are router calls of this session.
    #[default]
    Offered,
    /// The agent has its own tools and the host's `fs/*` and `terminal/*` only.
    Off,
}

/// A preset that confines what a program brings of its own (its account's connectors, skills and
/// plugins, its own permission prompt for the desktop's tools). Docket sends the settings in the
/// `session/new` it writes and sets the variables the preset names; the program cannot change
/// either. See `managed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum Profile {
    /// Claude Code: settings in `session/new` `_meta` and the variables of its isolated mode.
    #[serde(rename = "claude-code")]
    ClaudeCode,
}

/// Which model the endpoint serves.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    /// `account` (an API-key account) or `model` (a model on this computer).
    pub kind: EndpointKind,
    /// The account id, or the catalogue model id.
    pub id: String,
    /// Model ids the agent may name; empty means any.
    #[serde(default)]
    pub models: Vec<String>,
}

/// What an endpoint route points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointKind {
    /// An API-key account at a provider.
    Account,
    /// A model on this computer or an attached engine.
    Model,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    program: String,
    command: Option<String>,
    registry: Option<String>,
    version: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    route: Option<Route>,
    #[serde(default)]
    network: NetworkMode,
    #[serde(default)]
    allow: Vec<String>,
    #[serde(default)]
    deny: Vec<String>,
    #[serde(default)]
    ports: Vec<u16>,
    key_env: Option<String>,
    base_url_env: Option<String>,
    protocol: Option<AgentProtocol>,
    class: Option<DataClass>,
    #[serde(default)]
    delivery: Delivery,
    endpoint: Option<Endpoint>,
    #[serde(default)]
    reads: Vec<String>,
    #[serde(default)]
    state: Vec<String>,
    home: Option<String>,
    #[serde(default)]
    set: BTreeMap<String, String>,
    #[serde(default)]
    login: Vec<String>,
    #[serde(default)]
    logout: Vec<String>,
    #[serde(default)]
    tools: ToolsMode,
    label: Option<String>,
    profile: Option<Profile>,
    sign_in: Option<String>,
    model: Option<String>,
    checkpoints: Option<Rewind>,
    resume: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    agent: Vec<Raw>,
}

/// Why `agents.toml` was not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigFault {
    /// Not TOML, or a field of the wrong type or an unknown one.
    #[error("agents.toml is not valid: {0}")]
    Syntax(String),
    /// An entry is wrong; the program (as written) and what.
    #[error("agent `{program}`: {why}")]
    Entry {
        /// The program as written.
        program: String,
        /// What is wrong.
        why: &'static str,
    },
    /// An `allow` or `deny` entry that is not a host or a `*.domain` pattern.
    #[error("agent `{program}`: `{entry}` in {list} is not a usable site: {why}")]
    Site {
        /// The program as written.
        program: String,
        /// Which list: `allow` or `deny`.
        list: &'static str,
        /// The entry as written.
        entry: String,
        /// What is wrong with it.
        why: HostFault,
    },
    /// Two entries for one program.
    #[error("agent `{0}` is listed twice")]
    Twice(String),
    /// A registry agent that cannot be started from the agents directory.
    #[error("agent `{program}`: {why}")]
    Registry {
        /// The program as written.
        program: String,
        /// What is wrong.
        why: LaunchFault,
    },
}

/// One listed program, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// Our name for it (grants and records use this).
    pub name: ProgramName,
    /// Porter's name for it (the same text, in porter's grammar).
    pub program: AgentProgram,
    /// The executable, absolute.
    pub command: AbsPath,
    /// Its arguments.
    pub args: Vec<String>,
    /// How it gets a model.
    pub route: Route,
    /// The network it gets.
    pub network: NetworkMode,
    /// The sites it may reach: set exactly when `network` is `Allowlist`.
    pub reach: Option<Reach>,
    /// The variable it reads a key from.
    pub key_env: Option<EnvName>,
    /// The variable that overrides its model endpoint.
    pub base_url_env: Option<EnvName>,
    /// The protocol it speaks to a model.
    pub protocol: AgentProtocol,
    /// The data class the route carries.
    pub class: DataClass,
    /// How a handed-off key travels.
    pub delivery: Delivery,
    /// The endpoint route's target.
    pub endpoint: Option<Endpoint>,
    /// Read-only paths (the program's installation).
    pub reads: Vec<AbsPath>,
    /// Read-write paths (its own login and settings): per program, never shared.
    pub state: Vec<AbsPath>,
    /// What `HOME` is inside the sandbox; none means an empty temporary one.
    pub home: Option<AbsPath>,
    /// Plain variables to set (never a secret).
    pub set: Vec<(EnvName, String)>,
    /// The visible login command, if the program has one.
    pub login: Vec<String>,
    /// The logout command, if it has one.
    pub logout: Vec<String>,
    /// Whether the agent is offered the desktop's actions.
    pub tools: ToolsMode,
    /// What the person calls it, shown in the audit and the journal. Written here and nowhere
    /// else: never taken from what the agent says of itself.
    pub label: Option<prov::AgentLabel>,
    /// The preset that confines the program's own extras, if the person named one.
    pub profile: Option<Profile>,
    /// The way the agent signs itself in before a session opens, one of the ids it advertises.
    /// It signs in from the login it already holds; docket sends the id and nothing else.
    pub sign_in: Option<SignIn>,
    /// The model to switch to after the session opens, one the agent offers.
    pub model: Option<ModelId>,
    /// The registry id it was installed under, when it came from the registry: its record of
    /// what it offered is kept under that name.
    pub registry: Option<String>,
    /// Who keeps the history of the files it changes (`checkpoints`): `Docket` saves a restore
    /// point before each turn, `Agent` leaves it to the agent's own. Unwritten, it is `Agent`
    /// for the `claude-code` profile (whose own rewind the person expects) and `Docket` for any
    /// other. The host passes it to the router when the session opens; the agent never says it.
    pub rewind: Rewind,
    /// The arguments that make the app's own command-line program pick one conversation up,
    /// with `{session}` standing for the app's own id of it; none when the app has no such
    /// flag. Unwritten, it is `--resume {session}` for the `claude-code` profile.
    pub resume: Option<Vec<String>>,
}

/// What stands for the app's own id of a conversation in [`Entry::resume`].
pub const SESSION_MARK: &str = "{session}";

impl Entry {
    /// The arguments that make the app's own command-line program pick the conversation `own`
    /// up (the app's own id for it, put where `{session}` stands); none when the entry has no
    /// such flag.
    pub fn resume_line(&self, own: &str) -> Option<Vec<String>> {
        let args = self.resume.as_ref()?;
        Some(
            args.iter()
                .map(|arg| arg.replace(SESSION_MARK, own))
                .collect(),
        )
    }
}

/// Every listed program, by name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AgentsFile {
    entries: BTreeMap<AgentProgram, Entry>,
}

impl AgentsFile {
    /// Reads the text of `agents.toml`. Any wrong entry refuses the whole file: a half-read
    /// list of what may run is worse than none.
    pub fn parse(text: &str) -> Result<Self, ConfigFault> {
        Self::parse_in(text, None)
    }

    /// Like `parse`, with the agents directory a registry agent is installed in.
    pub fn parse_in(text: &str, dir: Option<&AgentsDir>) -> Result<Self, ConfigFault> {
        let file: File =
            toml::from_str(text).map_err(|e| ConfigFault::Syntax(e.message().to_owned()))?;
        let mut entries = BTreeMap::new();
        for raw in file.agent {
            let written = raw.program.clone();
            let entry = check::check(raw, dir)?;
            if entries.insert(entry.program.clone(), entry).is_some() {
                return Err(ConfigFault::Twice(written));
            }
        }
        Ok(Self { entries })
    }

    /// The entry for `name`, if listed.
    pub fn get(&self, name: &ProgramName) -> Option<&Entry> {
        self.entries.values().find(|e| &e.name == name)
    }

    /// The entry for the porter-side name.
    pub fn by_program(&self, program: &AgentProgram) -> Option<&Entry> {
        self.entries.get(program)
    }

    /// Every entry, in the order of their programs.
    pub(crate) fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.values()
    }

    /// The programs, for `Launcher::register`.
    pub fn programs(&self) -> Vec<AgentProgram> {
        self.entries.keys().cloned().collect()
    }

    /// How many are listed.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether none are.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
