//! Whether the edge is on: the setting `agent.mcp.expose` (quire design/22 section 3.27, owner
//! docket, page Intelligence > Privacy). MCP is off by default (QUESTIONS S7): an edge that was
//! never switched on lists no tools and refuses every call.
//!
//! The type is `docket-settings`', which reads the setting; nothing here reads a file or the
//! environment.

pub use docket_settings::McpExpose;
