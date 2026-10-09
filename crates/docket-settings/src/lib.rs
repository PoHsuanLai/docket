//! docket's settings (design/22 section 3.27): the `agent.*` keys the Settings app writes to
//! `$XDG_CONFIG_HOME/docket/settings.toml`, the schema it draws them from, and the lenient reader
//! the daemons share. A bad value falls back per key and is reported; the file is never fatal.
//! Reading is a pure function of text; only [`Locator::read`] touches the disk, and the live
//! watch is the daemon's (intentd), so this crate reaches no runtime and no watcher.

mod expose;
mod keys;
mod locate;
mod places;
mod read;
#[cfg(test)]
mod tests;

use docket_core::AgentConfig;

pub use expose::{AcpAgents, AcpExpose, McpExpose};
pub use locate::Locator;
pub use places::*;
pub use read::{Fallback, Loaded, Why, read};

/// The schema docket ships for its settings (design/22 section 9.2).
pub const SCHEMA: &str = include_str!("../../../dist/settings/docket.settings.toml");

/// The file the settings live in, under the configuration directory.
pub const SETTINGS_FILE: &str = "docket/settings.toml";

/// The reviewers' own timeouts when the router enforces the person's: the top of the ranges, so
/// the router's live deadline is the one that fires.
pub const REVIEW_CEILING: docket_core::ReviewTimeouts = docket_core::ReviewTimeouts {
    quick: docket_core::Millis(10_000),
    deliberate: docket_core::Millis(30_000),
    second: docket_core::Millis(30_000),
};

/// Every value of the file, typed: what the router and the companion read, and whether the MCP
/// and ACP edges are on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentSettings {
    /// The proposed values, with the person's choices in them.
    pub agent: AgentConfig,
    /// `agent.mcp.expose`.
    pub expose: McpExpose,
    /// `agent.acp.expose`.
    pub acp: AcpExpose,
    /// `agent.acp.agents`.
    pub agents: AcpAgents,
}

impl AgentSettings {
    /// The settings of a machine with no file: `agent`'s values, the edge off.
    pub fn over(agent: AgentConfig) -> Self {
        Self {
            agent,
            expose: McpExpose::Off,
            acp: AcpExpose::Off,
            agents: AcpAgents::Off,
        }
    }
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self::over(AgentConfig::default())
    }
}
