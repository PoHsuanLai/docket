//! The per-session tool edge (design note D-3): how an external agent reaches the desktop's
//! actions without a second way past the router.
//!
//! The host opens the agent's session at the router, then makes a unix socket for that session
//! alone and offers the agent an MCP server (`session/new` `mcpServers`, stdio form) that is
//! `actions-mcp --host-socket <socket>` with the session's token in its environment. The agent
//! starts that process itself, inside its sandbox, where the socket and the program are bound in.
//! The process holds no authority: it forwards `tools/list` and `tools/call` over the socket, and
//! the host makes each call as a router call in the session it opened, as the program it named,
//! exactly as it makes a file read. The tool names, schemas, argument reading and outcomes are
//! `actions-tools`', the same as the MCP edge's.
//!
//! - `ToolsOffer`: what the host is given to offer (a directory for sockets, the bridge program).
//! - `ToolsEdge`: the listening side of one session. Dropped, it stops, and its socket is gone.
//! - `EdgeBind`: what the launcher binds into the agent's sandbox for it.
//!
//! The binding is the host's, never the agent's: a request has no field that names a session,
//! program or app (`actions_tools::EdgeRequest`), the socket serves one session, and the token
//! is that session's. See FINDINGS, "acp-edge", for the threat model.

mod offer;
mod serve;
mod token;

pub use offer::{EdgeBind, ToolsOffer};
pub use serve::{EdgeFault, ToolsEdge};
