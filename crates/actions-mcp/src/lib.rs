//! The MCP edge: an MCP server generated from the registry's offered actions, for external
//! clients (a coding agent, a desktop assistant). Off by default (QUESTIONS S7). Every call goes
//! through the router as `Actor::Mcp`; every argument a client sends is labelled `Untrusted`
//! from `Source::Mcp(client)`, so an MCP client gets `Allow` for reads and a confirmation for
//! everything else. Importing third-party MCP tools into the registry is later work.

mod edge;
mod label;
mod tools;

pub use edge::McpEdge;
pub use label::mcp_label;
pub use tools::{McpFault, McpTool, McpToolName, ToolHints, hints_of, offered, tool_name, tools};
