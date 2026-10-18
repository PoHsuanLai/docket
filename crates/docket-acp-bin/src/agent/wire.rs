//! Building a host's wiring from this computer: the settings and `agents.toml` that allow an
//! agent to run, the sandbox, the bus and the launcher on it. `docket-agent` on a terminal and
//! its service (`serve`) both start from here, so the two hold an agent to the same rules.

use super::fault::AgentFault;
use super::host::Wiring;
use super::{agents_dir, config_dir, data_dir, env, sibling};
use bulkhead::Detected;
use docket_acp::client::{Fallback, ToolsOffer};
use docket_dbus::BusConnection;
use docket_dbus::callers::Identity;
use docket_inapp::EditorDesk;
use docket_launch::dbus::DbusAccounts;
use docket_launch::login::VisibleLogin;
use docket_launch::{AgentsFile, AgentsPermit, ChildProc, Registry, Supervisor};
use docket_settings::{AgentSettings, Locator};
use std::path::PathBuf;
use std::sync::Arc;

/// Whether the agent is offered the desktop's actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tools {
    /// When `actions-mcp` is beside this program; an entry in `agents.toml` can still switch it
    /// off for its program.
    Wanted,
    /// Never: a start that only reads what the agent offers.
    Unwanted,
}

/// What `agent.acp.agents` and `agents.toml` say, before anything starts: the proof the setting
/// is on, and the programs that may start.
#[derive(Debug)]
pub struct Allowed {
    /// The proof that `agent.acp.agents` is on.
    pub permit: AgentsPermit,
    /// The programs that may start.
    pub file: Arc<AgentsFile>,
}

/// Reads the setting and `agents.toml`. Off, or no file, or a file that is wrong, is a fault.
pub fn allowed() -> Result<Allowed, AgentFault> {
    let loaded = Locator::from_env(&env).read(AgentSettings::default());
    let permit = AgentsPermit::from_setting(loaded.value.agents)
        .map_err(|e| AgentFault::Permit(e.to_string()))?;
    let text = std::fs::read_to_string(config_dir().join("docket/agents.toml"))
        .map_err(|_| AgentFault::NoAgentsFile)?;
    let agents = agents_dir();
    let file = Arc::new(
        AgentsFile::parse_in(&text, Some(&agents))
            .map_err(|e| AgentFault::Agents(e.to_string()))?,
    );
    Ok(Allowed { permit, file })
}

/// What runs an agent's own login or logout command: given the variables it needs to show
/// itself, chosen here because the launcher crate reads no environment.
pub fn login_runner() -> VisibleLogin {
    let vars = [
        "HOME",
        "PATH",
        "DISPLAY",
        "WAYLAND_DISPLAY",
        "XDG_RUNTIME_DIR",
        "DBUS_SESSION_BUS_ADDRESS",
    ];
    VisibleLogin::new(
        vars.iter()
            .filter_map(|k| env(k).map(|v| ((*k).to_owned(), v)))
            .collect(),
    )
}

/// The wiring of a host on `bus`: the sandbox, the launcher registered on the bus and the tool
/// edge. `fallback` says who answers the sheets of the agent's session.
pub async fn build(
    allowed: Allowed,
    tools: Tools,
    fallback: Fallback,
    bus: BusConnection,
    limits: SandboxLimits,
) -> Result<Wiring<DbusAccounts>, AgentFault> {
    let Allowed { permit, file } = allowed;
    let path = env("PATH").unwrap_or_default();
    let bwrap = match limits.probe(&path) {
        Detected::Bwrap(bwrap) => bwrap,
        Detected::Missing(why) => return Err(AgentFault::NoSandbox(why)),
    };
    let forwarder = sibling("docket-net-forward").ok_or(AgentFault::NoForwarder)?;
    // Without the exec stage the agent is held by the sandbox alone: say so, never hide it.
    let enter = sibling("docket-net-enter").filter(|p| std::path::Path::new(p.as_str()).exists());
    if enter.is_none() {
        eprintln!(
            "docket-agent: no docket-net-enter beside this program: the kernel does not limit the agent's files or network"
        );
    }
    let run_dir = PathBuf::from(env("XDG_RUNTIME_DIR").ok_or(AgentFault::NoRuntimeDir)?);
    // The desktop's actions go to the agent as an MCP server it starts itself: the `actions-mcp`
    // program beside this one, as the bridge to this host. Whether a program is offered it (an
    // entry can switch it off) and which of the actions is decided where its backend is made.
    let offered = (tools == Tools::Wanted)
        .then(|| sibling("actions-mcp"))
        .flatten()
        .filter(|bridge| std::path::Path::new(bridge.as_str()).exists())
        .map(|bridge| ToolsOffer {
            run_dir: run_dir.clone(),
            bridge,
        });
    if offered.is_none() && tools == Tools::Wanted {
        eprintln!(
            "docket-agent: no agent is offered the desktop's actions (no actions-mcp beside this program)"
        );
    }

    let table = crate::callers::read(&env).map_err(|e| AgentFault::Callers(e.to_string()))?;
    let callers = Arc::new(Identity::on_system(bus.clone(), table));
    let accounts = Arc::new(
        DbusAccounts::connect(&bus)
            .await
            .map_err(|e| AgentFault::Bus(e.to_string()))?,
    );
    let registry: Registry<ChildProc> = Registry::default();
    let supervisor = Supervisor::new(
        accounts.clone(),
        file.clone(),
        registry.clone(),
        login_runner(),
    );
    supervisor
        .register()
        .await
        .map_err(|e| AgentFault::Bus(e.to_string()))?;
    tokio::spawn(async move { supervisor.run().await });

    if data_dir().join("acp-agent-grants.json").exists() {
        eprintln!(
            "docket-agent: ignoring the old acp-agent-grants.json in the data directory: \
             an agent's grants are docket's now (Settings lists and revokes them); allow \
             \"always\" again where you want one"
        );
    }
    Ok(Wiring {
        bus,
        accounts,
        file,
        permit,
        bwrap: Detected::Bwrap(bwrap),
        forwarder,
        enter,
        run_dir,
        registry,
        tools: offered,
        fallback,
        desk: EditorDesk::new(),
        callers,
        agents: Some(agents_dir()),
    })
}

/// How the sandbox bounds what an agent uses.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SandboxLimits {
    /// A scope of the person's systemd user manager holds the agent's memory and tasks: what the
    /// packaged program always uses.
    #[default]
    Scope,
    /// Per-process limits instead, for a machine with no user manager. Only the acceptance
    /// harness's main passes it (its jail runs no systemd).
    PerProcess,
}

impl SandboxLimits {
    /// The sandbox here, bounded this way.
    fn probe(self, path: &str) -> Detected {
        match self {
            SandboxLimits::Scope => Detected::probe(path),
            SandboxLimits::PerProcess => {
                let hardening = bulkhead::Hardening {
                    limits: bulkhead::LimitPolicy::Rlimit(bulkhead::Limits::default()),
                    ..bulkhead::Hardening::default()
                };
                Detected::probe_with(path, &hardening, &bulkhead::UserSession::from_env())
            }
        }
    }
}
