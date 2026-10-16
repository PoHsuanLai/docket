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
use docket_launch::{AgentsFile, AgentsPermit, ChildProc, Registry, Supervisor, ToolsMode};
use docket_session::ProgramName;
use docket_settings::{AgentSettings, Locator};
use std::path::PathBuf;
use std::sync::Arc;

/// Whether the agent is offered the desktop's actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tools {
    /// When its entry in `agents.toml` allows it and `actions-mcp` is beside this program.
    Wanted,
    /// Never: a start that only reads what the agent offers.
    Unwanted,
}

/// What `agent.acp.agents` and `agents.toml` say, before anything starts: the proof the setting
/// is on, and the programs that may start.
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

/// The wiring of a host for `program` on `bus`: the sandbox, the launcher registered on the bus
/// and the tool edge. `fallback` says who answers the sheets of the agent's session.
pub async fn build(
    allowed: Allowed,
    program: &ProgramName,
    tools: Tools,
    fallback: Fallback,
    bus: BusConnection,
) -> Result<Wiring<DbusAccounts>, AgentFault> {
    let Allowed { permit, file } = allowed;
    let path = env("PATH").unwrap_or_default();
    let Detected::Bwrap(bwrap) = Detected::probe(&path) else {
        return Err(AgentFault::NoSandbox);
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
    // program beside this one, as the bridge to this host. An entry can switch it off.
    let offered = file
        .get(program)
        .filter(|entry| entry.tools == ToolsMode::Offered && tools == Tools::Wanted)
        .and_then(|_| sibling("actions-mcp"))
        .filter(|bridge| std::path::Path::new(bridge.as_str()).exists())
        .map(|bridge| ToolsOffer {
            run_dir: run_dir.clone(),
            bridge,
        });
    if offered.is_none() && tools == Tools::Wanted {
        eprintln!(
            "docket-agent: the agent is not offered the desktop's actions (off, or no actions-mcp beside this program)"
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
