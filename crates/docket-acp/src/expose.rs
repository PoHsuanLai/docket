//! Whether the edge may serve at all: the setting `agent.acp.expose`, off unless the file says
//! `on`. A `Permit` is the proof that it was read as on; the server cannot be built without one,
//! so a binary that was never switched on has nothing to serve with.

use docket_settings::{AcpExpose, AgentSettings, Locator, read};

/// Why the edge will not serve.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
    /// `agent.acp.expose` is off (or unreadable, which is the same).
    #[error("agent.acp.expose is off: docket-acp refuses to serve")]
    Off,
}

/// The right to serve, from reading the setting as on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permit(());

impl Permit {
    /// From the setting's value.
    pub fn from_setting(expose: AcpExpose) -> Result<Self, Refusal> {
        match expose {
            AcpExpose::On => Ok(Permit(())),
            AcpExpose::Off => Err(Refusal::Off),
        }
    }

    /// From the text of a settings file; anything but `on` refuses.
    pub fn from_text(text: &str) -> Result<Self, Refusal> {
        Self::from_setting(read(text, AgentSettings::default()).value.acp)
    }

    /// From the first settings file of the configuration directories; none refuses.
    pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> Result<Self, Refusal> {
        let loaded = Locator::from_env(env).read(AgentSettings::default());
        Self::from_setting(loaded.value.acp)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_on_permits() {
        let cases = [
            ("", false),
            ("[agent.acp]\nexpose = \"on\"\n", true),
            ("[agent.acp]\nexpose = \"off\"\n", false),
            ("[agent.acp]\nexpose = \"maybe\"\n", false),
            ("[agent.acp]\nexpose = true\n", false),
            ("[agent.mcp]\nexpose = \"on\"\n", false),
            ("[agent.acp\nexpose = \"on\"", false),
        ];
        for (text, want) in cases {
            assert_eq!(Permit::from_text(text).is_ok(), want, "{text:?}");
        }
    }
}
