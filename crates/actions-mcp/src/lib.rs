//! The MCP edge: an MCP server generated from the registry's offered actions, for external
//! clients (a coding agent, a desktop assistant). Off by default (QUESTIONS S7). Every call goes
//! through the router as `Actor::Mcp`; every argument a client sends is labelled `Untrusted`
//! from `Source::Mcp(client)`, so an MCP client gets `Allow` for reads and a confirmation for
//! everything else. Importing third-party MCP tools into the registry is later work.
//!
//! - `McpAccess`: the switch. `McpEdge::new` is `Off`.
//! - `read_call` (`args`): JSON arguments and the `target` key to typed ones, by declared type.
//! - `McpEdge::call` and its `ServerHandler`: one tool call through `Intents::perform`.
//! - `McpFault`: why a call gave nothing, coarse.

mod access;
mod args;
mod edge;
mod fault;
mod label;
mod result;
mod tools;

pub use access::McpAccess;
pub use args::{TARGET_KEY, read_call, target};
pub use edge::McpEdge;
pub use fault::{ArgsFault, McpFault, McpRefusal, TargetFault, Why};
pub use label::mcp_label;
pub use result::outcome_json;
pub use tools::{McpTool, McpToolName, ToolHints, hints_of, offered, tool_name, tools};
