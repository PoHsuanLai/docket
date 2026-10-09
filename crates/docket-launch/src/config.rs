//! The agent programs the person lists in `agents.toml`: what to run, how it takes a model
//! endpoint or a key, what it may see and write, and how much network it gets. Nothing starts
//! unless `agent.acp.agents` is on and the program is listed here; a prompt cannot add one.
//!
//! ```toml
//! [[agent]]
//! program = "claude-code"
//! command = "/home/me/.local/bin/claude-agent-acp"
//! route = "endpoint"                 # endpoint (inferd, P4) | handoff (a key, P2) | login
//! network = "endpoint_only"          # none | endpoint_only | host
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
//! [agent.endpoint]
//! kind = "account"
//! id = "anthropic-main"
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

use bulkhead::NetworkMode;
use docket_acp::client::{ModelId, SignIn};
use docket_agents::{AgentsDir, LaunchFault};
use docket_core::AbsPath;
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
/// (`docket_acp::client::ToolsEdge`).
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
}

/// The longest label, in characters.
const LABEL_MAX: usize = 64;

fn label(program: &str, text: Option<String>) -> Result<Option<prov::AgentLabel>, ConfigFault> {
    let Some(text) = text else { return Ok(None) };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(bad(program, "a label may not be empty"));
    }
    if trimmed.chars().count() > LABEL_MAX {
        return Err(bad(program, "a label is at most 64 characters"));
    }
    if trimmed.chars().any(char::is_control) {
        return Err(bad(
            program,
            "a label holds no control character or newline",
        ));
    }
    Ok(Some(prov::AgentLabel(trimmed.to_owned())))
}

fn bad(program: &str, why: &'static str) -> ConfigFault {
    ConfigFault::Entry {
        program: program.to_owned(),
        why,
    }
}

fn abs(program: &str, text: &str) -> Result<AbsPath, ConfigFault> {
    let path = AbsPath::parse(text).map_err(|_| bad(program, "a path must be absolute"))?;
    match path.is_root() {
        docket_core::RootState::Root => Err(bad(program, "a path must not be /")),
        docket_core::RootState::Below => Ok(path),
    }
}

fn env(program: &str, name: Option<&String>) -> Result<Option<EnvName>, ConfigFault> {
    name.map(|n| EnvName::parse(n).map_err(|_| bad(program, "not an environment variable name")))
        .transpose()
}

/// Variables the person's `set` table may not touch: the dynamic linker, the search path, and
/// anything that runs code before the program does.
fn forbidden(name: &str) -> bool {
    name.starts_with("LD_")
        || matches!(
            name,
            "PATH" | "HOME" | "BASH_ENV" | "ENV" | "IFS" | "PYTHONSTARTUP"
        )
        || name.starts_with("DOCKET_")
}

/// What starts the program and what it may see, from `command` or from the registry.
struct Source {
    command: AbsPath,
    args: Vec<String>,
    reads: Vec<AbsPath>,
    state: Vec<AbsPath>,
    home: Option<AbsPath>,
    set: Vec<(String, String)>,
}

fn source(at: &str, raw: &Raw, dir: Option<&AgentsDir>) -> Result<Source, ConfigFault> {
    let paths = |list: &[String]| -> Result<Vec<AbsPath>, ConfigFault> {
        list.iter().map(|p| abs(at, p)).collect()
    };
    let args = raw.args.clone();
    let set = raw.set.clone().into_iter().collect::<Vec<_>>();
    match (&raw.command, &raw.registry, &raw.version) {
        (Some(command), None, None) => Ok(Source {
            command: abs(at, command)?,
            args,
            reads: paths(&raw.reads)?,
            state: paths(&raw.state)?,
            home: raw.home.as_deref().map(|p| abs(at, p)).transpose()?,
            set,
        }),
        (None, Some(_), Some(_)) if !raw.state.is_empty() || raw.home.is_some() => Err(bad(
            at,
            "a registry agent has a home of its own: no state or home",
        )),
        (None, Some(id), Some(version)) => {
            let dir = dir.ok_or_else(|| bad(at, "a registry agent needs the agents directory"))?;
            let launch = dir
                .launch(id, version)
                .map_err(|why| ConfigFault::Registry {
                    program: at.to_owned(),
                    why,
                })?;
            let path = |p: &std::path::Path| {
                p.to_str()
                    .ok_or_else(|| bad(at, "the agents directory is not UTF-8"))
                    .and_then(|t| abs(at, t))
            };
            let home = path(&launch.home)?;
            let reads = [path(&launch.files)?]
                .into_iter()
                .chain(paths(&raw.reads)?)
                .collect();
            Ok(Source {
                command: path(&launch.command)?,
                args: launch.args.into_iter().chain(args).collect(),
                reads,
                state: vec![home.clone()],
                home: Some(home),
                set: launch.env.into_iter().chain(set).collect(),
            })
        }
        (None, Some(_), None) => Err(bad(at, "a registry agent needs a pinned version")),
        _ => Err(bad(
            at,
            "name either command, or registry with version (not both)",
        )),
    }
}

fn check(raw: Raw, dir: Option<&AgentsDir>) -> Result<Entry, ConfigFault> {
    let at = raw.program.clone();
    let name = ProgramName::parse(&raw.program).map_err(|_| bad(&at, "not a program name"))?;
    let program = AgentProgram::parse(&raw.program).map_err(|_| {
        bad(
            &at,
            "a program is lower-case letters, digits and - (porter's grammar)",
        )
    })?;
    let src = source(&at, &raw, dir)?;
    let route = raw
        .route
        .or_else(|| raw.registry.as_ref().map(|_| Route::Login))
        .ok_or_else(|| bad(&at, "route is required"))?;
    if src.args.iter().any(|a| a.contains('\0')) {
        return Err(bad(&at, "an argument holds a NUL"));
    }
    let label = label(&at, raw.label.clone())?;
    let key_env = env(&at, raw.key_env.as_ref())?;
    let base_url_env = env(&at, raw.base_url_env.as_ref())?;
    let set = src
        .set
        .iter()
        .map(|(k, v)| {
            let name =
                EnvName::parse(k).map_err(|_| bad(&at, "not an environment variable name"))?;
            let fine = !forbidden(k)
                && Some(&name) != key_env.as_ref()
                && Some(&name) != base_url_env.as_ref()
                && !v.chars().any(char::is_control);
            if fine {
                Ok((name, v.clone()))
            } else {
                Err(bad(
                    &at,
                    "a `set` variable may not be a key, a base URL, PATH, HOME or a loader switch",
                ))
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    // The route decides what the entry must say, and what network makes sense.
    match (route, raw.network) {
        (Route::Endpoint, NetworkMode::None) => {
            return Err(bad(
                &at,
                "the endpoint route needs endpoint_only (or host) network",
            ));
        }
        (Route::Handoff, NetworkMode::None | NetworkMode::EndpointOnly) => {
            return Err(bad(
                &at,
                "a handed-off key is for a program that reaches its provider: network host",
            ));
        }
        (_, NetworkMode::EndpointOnly) if route != Route::Endpoint => {
            return Err(bad(&at, "endpoint_only is for the endpoint route"));
        }
        _ => {}
    }
    if route == Route::Endpoint && (raw.base_url_env.is_none() || raw.key_env.is_none()) {
        return Err(bad(
            &at,
            "the endpoint route needs base_url_env and key_env",
        ));
    }
    if route == Route::Endpoint && raw.endpoint.is_none() {
        return Err(bad(
            &at,
            "the endpoint route needs an [agent.endpoint] table",
        ));
    }
    if route == Route::Handoff && raw.key_env.is_none() {
        return Err(bad(&at, "the handoff route needs key_env"));
    }
    Ok(Entry {
        name,
        program,
        command: src.command,
        args: src.args,
        route,
        network: raw.network,
        key_env,
        base_url_env,
        protocol: raw.protocol.unwrap_or(AgentProtocol::AnthropicMessages),
        class: raw.class.unwrap_or(DataClass::Files),
        delivery: raw.delivery,
        endpoint: raw.endpoint,
        reads: src.reads,
        state: src.state,
        home: src.home,
        set,
        login: raw.login,
        logout: raw.logout,
        tools: raw.tools,
        label,
        profile: raw.profile,
        sign_in: raw
            .sign_in
            .as_deref()
            .map(SignIn::parse)
            .transpose()
            .map_err(|_| bad(&at, "sign_in is 1 to 64 letters, digits, - _ or ."))?,
        model: raw
            .model
            .as_deref()
            .map(ModelId::parse)
            .transpose()
            .map_err(|_| bad(&at, "a model id has no spaces or control characters"))?,
        registry: raw.registry.clone(),
    })
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
            let entry = check(raw, dir)?;
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
