//! How far a running agent is held, in the words a person reads. Two facts make it: what the
//! kernel limits (the level the exec stage reported, or none when there is no exec stage) and
//! which network the agent was given. Nothing here names how it is done.

use bulkhead::{LandlockLevel, NetworkMode};

/// The confinement of one agent process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confinement {
    level: Option<LandlockLevel>,
    net: NetworkMode,
    sites: Vec<String>,
}

impl Confinement {
    /// `level` is what the exec stage reported, `None` when it did not (not installed, or not
    /// yet heard from); that is shown as not limited, never left out. `sites` are the allowed
    /// sites of an allowlist network.
    pub fn new(level: Option<LandlockLevel>, net: NetworkMode, sites: Vec<String>) -> Self {
        Self { level, net, sites }
    }

    /// The one-line summary. The network counts as limited by the kernel only when the exec
    /// stage reached its full level and the network is not the host's.
    pub fn summary(&self) -> &'static str {
        match (self.level, self.net) {
            (
                Some(LandlockLevel::Full),
                NetworkMode::None | NetworkMode::EndpointOnly | NetworkMode::Allowlist,
            ) => "Files and network limited",
            (Some(LandlockLevel::Full | LandlockLevel::FilesOnly), _) => {
                "Files limited, network not limited by the kernel"
            }
            _ => "Not limited by the kernel",
        }
    }

    /// What the agent can reach.
    pub fn network(&self) -> String {
        match self.net {
            NetworkMode::None => "Cannot reach the network".to_owned(),
            NetworkMode::EndpointOnly => "Can reach only its model".to_owned(),
            NetworkMode::Allowlist => format!("Can reach only: {}", self.sites.join(", ")),
            NetworkMode::Host => "Can reach the whole network".to_owned(),
            _ => "Network not limited".to_owned(),
        }
    }
}
