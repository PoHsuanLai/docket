//! The MCP edge: an MCP server generated from the registry's offered actions, for external
//! clients (a coding agent, a desktop assistant). Off by default (QUESTIONS S7). Every call goes
//! through the router as `Actor::Mcp`; every argument a client sends is labelled `Untrusted`
//! from `Source::Mcp(client)`, so an MCP client gets `Allow` for reads and a confirmation for
//! everything else. Importing third-party MCP tools into the registry is later work.
//!
//! - `McpExpose`: the switch. `McpEdge::new` is `Off`.
//! - `read_call` (`args`): JSON arguments and the `target` key to typed ones, by declared type.
//! - `McpEdge::call` and its `ServerHandler`: one tool call through `Intents::perform`.
//! - `McpFault`: why a call gave nothing, coarse.

mod args;
mod config;
mod daemon;
mod edge;
mod expose;
mod fault;
mod label;
mod result;
mod settings;
mod tools;

pub use args::{TARGET_KEY, read_call, target};
pub use config::{ConfigError, McpConfig};
pub use daemon::{
    Args, DaemonFault, ExposeRead, Listen, MCP_BUS, claim, run, start, start_following,
    write_schema,
};
pub use edge::McpEdge;
pub use expose::McpExpose;
pub use fault::{ArgsFault, McpFault, McpRefusal, TargetFault, Why};
pub use label::mcp_label;
pub use result::outcome_json;
pub use settings::{SCHEMA, SETTINGS_FILE, SETTINGS_KEY, exposed};
pub use tools::{McpTool, McpToolName, ToolHints, hints_of, offered, tool_name, tools};
