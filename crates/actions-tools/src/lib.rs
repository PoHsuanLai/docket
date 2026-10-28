//! The tools a registry offers to an outside agent, pure: no runtime, no bus, no MCP SDK.
//!
//! - `tools`, `offered`, `tool_name`, `tool_of`: which actions are offered (`docket_manifest::is_offered`)
//!   and how each is named and described.
//! - `listing`, `prepare`: an edge's tool list and one tool call made into a request, the one copy
//!   every edge uses.
//! - `tool_label`: what every argument from such an agent is worth (`Untrusted`).
//! - `outcome_json`, `ToolRefusal`, `ToolFault`: what comes back, coarse on purpose.
//!
//! `actions-mcp` serves these over MCP for the clients that connect to it; `docket-acp` serves the
//! same over the per-session edge of an external ACP agent. Each keeps only its transport.

mod fault;
mod label;
mod prepare;
mod result;
mod session_edge;
mod tools;

pub use fault::{ToolFault, ToolRefusal};
pub use label::tool_label;
pub use prepare::{listing, prepare};
pub use result::outcome_json;
pub use session_edge::{
    EDGE_LINE_MAX, EDGE_SOCKET_ENV, EDGE_TOKEN_ENV, EdgeOp, EdgeReply, EdgeRequest, EdgeToken,
    EdgeTool,
};
pub use tools::{
    ToolHints, ToolName, ToolSpec, find, hints_of, offered, tool_name, tool_of, tools,
};
