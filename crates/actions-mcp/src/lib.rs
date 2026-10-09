//! The MCP edge: an MCP server generated from the registry's offered actions, for external
//! clients (a coding agent, a desktop assistant). Off by default (QUESTIONS S7). Every call goes
//! through the router as `Actor::Mcp`; every argument a client sends is labelled `Untrusted`
//! from `Source::Mcp(client)`, so an MCP client gets `Allow` for reads and a confirmation for
//! everything else. Importing third-party MCP tools into the registry is later work.
//!
//! - `McpExpose`: the switch. `McpEdge::new` is `Off`.
//! - `read_call` (`args`): JSON arguments and the `target` key to typed ones, by declared type.
//! - `McpEdge::call` and its `ServerHandler`: one tool call through `Intents::perform`.
//! - `BoundEdge`: the bridge an external ACP agent starts (`--host-socket`), forwarding to the host
//!   that started the agent, which makes each call in the agent's own session.
//! - `McpFault`: why a call gave nothing, coarse.

mod bound;
mod config;
mod daemon;
mod edge;
mod expose;
mod settings;

pub use actions_tools::{
    ArgsFault, McpFault, McpRefusal, McpTool, McpToolName, TARGET_KEY, TargetFault, ToolHints, Why,
    hints_of, mcp_label, offered, outcome_json, read_call, target, tool_name, tools,
};
pub use bound::{BoundEdge, NoToken};
pub use config::{ConfigError, McpConfig};
pub use daemon::{
    Args, CommandLineFault, DaemonFault, ExposeRead, Listen, MCP_BUS, claim, run, start,
    start_following, write_schema,
};
pub use edge::McpEdge;
pub use expose::McpExpose;
pub use settings::{SCHEMA, SETTINGS_FILE, SETTINGS_KEY, exposed};
