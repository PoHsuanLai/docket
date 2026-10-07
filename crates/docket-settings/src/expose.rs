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

/// Whether a code editor may drive the companion over ACP: the setting `agent.acp.expose`. Off by
/// default, like the MCP edge: the `docket-acp` binary that was never switched on refuses to serve.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AcpExpose {
    /// The binary refuses to serve (the default).
    #[default]
    Off,
    /// An editor may open sessions and prompt.
    On,
}
