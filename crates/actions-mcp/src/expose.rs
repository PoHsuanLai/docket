//! Whether the edge is on: the setting `agent.mcp.expose` (quire design/22 section 3.27, owner
//! docket, page Intelligence > Privacy). MCP is off by default (QUESTIONS S7, actions.md section
//! 3.14): an edge that was never switched on lists no tools and refuses every call.
//!
//! The daemon that hosts the edge reads the setting (`settings.rs`) and hands the edge the
//! value. Nothing here reads a file or the environment.

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
