//! The right to start agent programs: `agent.acp.agents` read as on. No permit, no
//! `AgentSpawn`, so a build that was never switched on starts nothing.

use docket_settings::{AcpAgents, AgentSettings, read};

/// Why agents will not start.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// `agent.acp.agents` is off (or unreadable, which is the same).
    #[error("agent.acp.agents is off: no agent program starts")]
    Off,
}

/// The proof that the setting is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentsPermit(());

impl AgentsPermit {
    /// From the setting's value.
    pub fn from_setting(agents: AcpAgents) -> Result<Self, Refusal> {
        match agents {
            AcpAgents::On => Ok(Self(())),
            AcpAgents::Off => Err(Refusal::Off),
        }
    }

    /// From the text of the settings file; anything but `on` refuses.
    pub fn from_text(text: &str) -> Result<Self, Refusal> {
        Self::from_setting(read(text, AgentSettings::default()).value.agents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_permits() {
        for (text, want) in [
            ("", false),
            ("[agent.acp]\nagents = \"on\"\n", true),
            ("[agent.acp]\nagents = \"off\"\n", false),
            ("[agent.acp]\nagents = true\n", false),
            ("[agent.acp]\nexpose = \"on\"\n", false),
            ("[agent.acp\nagents = \"on\"", false),
        ] {
            assert_eq!(AgentsPermit::from_text(text).is_ok(), want, "{text:?}");
        }
    }
}
