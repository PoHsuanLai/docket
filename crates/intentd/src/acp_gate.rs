//! Whether the well-known name `org.quire.Acp` is anybody. `dist/intentd.toml` lists it as an editor
//! and a companion, but the process that owns it exists only while `agent.acp.expose` is `on`.
//! While the setting is off the name is usually unowned, so any process of the person could take it
//! and play both roles. The gate closes that: [`Peers`](crate::Peers) drops the name from a
//! connection's facts unless the setting is on, read at each call, so a flip of the setting
//! changes the next call and nothing is cached.
//!
//! The same for `org.quire.AcpAgent`, the host of an external coding agent (`acp_agent`): it is
//! anybody only while `agent.acp.agents` is `on`.
//!
//! This only narrows the exposure while the feature is off. Identity stays advisory for processes
//! of the same user (see `peer.rs`): with the setting on, any of them can still own the name.

use docket_settings::{AcpAgents, AcpExpose};
use porter_core::AppName;
use std::sync::Arc;
use tokio::sync::watch;

/// The bus name the ACP process owns.
pub const ACP_NAME: &str = "org.quire.Acp";

/// The settings as last read, shared by the settings watch (which sets them) and the peers (which
/// read them).
#[derive(Debug, Clone)]
pub struct AcpGate {
    expose: Arc<watch::Sender<AcpExpose>>,
    agents: Arc<watch::Sender<AcpAgents>>,
}

impl AcpGate {
    /// A gate showing `expose`.
    pub fn new(expose: AcpExpose) -> Self {
        Self {
            expose: Arc::new(watch::channel(expose).0),
            agents: Arc::new(watch::channel(AcpAgents::Off).0),
        }
    }

    /// A gate that is shut: the name is nobody. What a daemon that never read the setting uses.
    pub fn shut() -> Self {
        Self::new(AcpExpose::Off)
    }

    /// Puts a newly read setting in force for the next call.
    pub fn set(&self, expose: AcpExpose) {
        self.expose.send_replace(expose);
    }

    /// Puts a newly read `agent.acp.agents` in force for the next call.
    pub fn set_agents(&self, agents: AcpAgents) {
        self.agents.send_replace(agents);
    }

    /// The setting now.
    pub fn expose(&self) -> AcpExpose {
        *self.expose.borrow()
    }

    /// `agent.acp.agents` now.
    pub fn agents(&self) -> AcpAgents {
        *self.agents.borrow()
    }

    /// Whether a connection owning `name` keeps it: every name but the two ACP ones always; the
    /// editor server's only while `agent.acp.expose` is on, the agent host's only while
    /// `agent.acp.agents` is.
    pub(crate) fn keeps(&self, name: &AppName) -> bool {
        match name.as_str() {
            ACP_NAME => self.expose() == AcpExpose::On,
            docket_core::ACP_AGENT_APP => self.agents() == AcpAgents::On,
            _ => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> AppName {
        AppName::parse(text).expect("name")
    }

    #[test]
    fn only_the_acp_names_wait_on_their_settings() {
        let gate = AcpGate::shut();
        let host = name(docket_core::ACP_AGENT_APP);
        assert!(!gate.keeps(&name(ACP_NAME)));
        assert!(!gate.keeps(&host));
        assert!(gate.keeps(&name("org.quire.Shell")));
        gate.set(AcpExpose::On);
        assert!(gate.keeps(&name(ACP_NAME)));
        assert!(!gate.keeps(&host), "the editor setting is not the agents'");
        gate.set(AcpExpose::Off);
        assert!(!gate.keeps(&name(ACP_NAME)));
        gate.set_agents(AcpAgents::On);
        assert!(gate.keeps(&host));
        assert!(!gate.keeps(&name(ACP_NAME)));
    }

    #[test]
    fn a_clone_sees_the_same_setting() {
        let gate = AcpGate::shut();
        let seen = gate.clone();
        gate.set(AcpExpose::On);
        assert_eq!(seen.expose(), AcpExpose::On);
    }
}
