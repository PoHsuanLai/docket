//! The tools a registry offers to an outside agent, pure: no runtime, no bus, no MCP SDK.
//!
//! - `tools`, `offered`, `tool_name`, `tool_of`: which actions are offered and how each is named
//!   and described (only `AgentReach::Offered`, of apps that are not host-only).
//! - `read_call` (`args`): JSON arguments and the `target` key to typed ones, by declared type.
//! - `mcp_label`: what every argument from such an agent is worth (`Untrusted`).
//! - `outcome_json`, `McpRefusal`, `McpFault`: what comes back, coarse on purpose.
//!
//! `actions-mcp` serves these over MCP for the clients that connect to it; `docket-acp` serves the
//! same over the per-session edge of an external ACP agent. Keeping them here is what keeps the
//! two mappings one.

mod args;
mod fault;
mod label;
mod result;
mod session_edge;
mod tools;

pub use args::{TARGET_KEY, read_call, target};
pub use fault::{ArgsFault, McpFault, McpRefusal, TargetFault, Why};
pub use label::mcp_label;
pub use result::outcome_json;
pub use session_edge::{
    EDGE_LINE_MAX, EDGE_SOCKET_ENV, EDGE_TOKEN_ENV, EdgeOp, EdgeReply, EdgeRequest, EdgeToken,
    EdgeTool,
};
pub use tools::{
    McpTool, McpToolName, ToolHints, find, hints_of, offered, tool_name, tool_of, tools,
};
