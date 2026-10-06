//! Whether external MCP clients may use the registry's offered actions: the setting
//! `agent.mcp.expose` (design/22 section 3.27, page Intelligence > Privacy). MCP is off by default
//! (QUESTIONS S7): an edge that was never switched on lists no tools and refuses every call.

use serde::{Deserialize, Serialize};

/// Whether external clients may use the registry's offered actions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpExpose {
    /// No tools are listed and no call is made (the default).
    #[default]
    Off,
    /// The offered actions are tools.
    On,
}
