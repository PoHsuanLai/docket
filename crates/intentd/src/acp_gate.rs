//! Whether the well-known name `org.quire.Acp` is anybody. `dist/intentd.toml` lists it as an editor
//! and a companion, but the process that owns it exists only while `agent.acp.expose` is `on`.
//! While the setting is off the name is usually unowned, so any process of the person could take it
//! and play both roles. The gate closes that: [`Peers`](crate::Peers) drops the name from a
//! connection's facts unless the setting is on, read at each call, so a flip of the setting
//! changes the next call and nothing is cached.
//!
//! This only narrows the exposure while the feature is off. Identity stays advisory for processes
//! of the same user (see `peer.rs`): with the setting on, any of them can still own the name.

use docket_settings::AcpExpose;
use porter_core::AppName;
use std::sync::Arc;
use tokio::sync::watch;

/// The bus name the ACP process owns.
pub const ACP_NAME: &str = "org.quire.Acp";

/// The setting as last read, shared by the settings watch (which sets it) and the peers (which
/// read it).
#[derive(Debug, Clone)]
pub struct AcpGate {
    expose: Arc<watch::Sender<AcpExpose>>,
}

impl AcpGate {
    /// A gate showing `expose`.
    pub fn new(expose: AcpExpose) -> Self {
        Self {
            expose: Arc::new(watch::channel(expose).0),
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

    /// The setting now.
    pub fn expose(&self) -> AcpExpose {
        *self.expose.borrow()
    }

    /// Whether a connection owning `name` keeps it: every name but the ACP one always, that one
    /// only while the setting is on.
    pub(crate) fn keeps(&self, name: &AppName) -> bool {
        name.as_str() != ACP_NAME || self.expose() == AcpExpose::On
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> AppName {
        AppName::parse(text).expect("name")
    }

    #[test]
    fn only_the_acp_name_waits_on_the_setting() {
        let gate = AcpGate::shut();
        assert!(!gate.keeps(&name(ACP_NAME)));
        assert!(gate.keeps(&name("org.quire.Shell")));
        gate.set(AcpExpose::On);
        assert!(gate.keeps(&name(ACP_NAME)));
        gate.set(AcpExpose::Off);
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
