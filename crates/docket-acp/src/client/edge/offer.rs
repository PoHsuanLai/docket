//! What the host is given to offer an agent, and what the launcher binds for it.

use actions_tools::{EDGE_TOKEN_ENV, EdgeToken};
use agent_client_protocol_schema::v1::{EnvVariable, McpServer, McpServerStdio};
use docket_core::AbsPath;
use std::path::PathBuf;

/// The name the agent sees the server under.
pub const SERVER_NAME: &str = "quire";
/// The bridge program's flag that names the socket.
pub const SOCKET_FLAG: &str = "--host-socket";

/// What the host is given to offer: a directory only the user can enter for the sockets (such as
/// `$XDG_RUNTIME_DIR`), and the bridge program (`actions-mcp`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolsOffer {
    /// Where each session's socket directory is made.
    pub run_dir: PathBuf,
    /// The bridge program the agent will start.
    pub bridge: AbsPath,
}

/// What the launcher binds into the agent's sandbox so it can start the bridge and reach its
/// socket: the socket file (read and write) and the bridge program (read only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeBind {
    /// The session's socket.
    pub socket: AbsPath,
    /// The bridge program.
    pub bridge: AbsPath,
}

impl EdgeBind {
    /// The MCP server entry for `session/new`: the bridge, its socket, and the token in its
    /// environment (a command line is readable by every process of the user, an environment only
    /// by its owner).
    pub fn server(&self, token: &EdgeToken) -> McpServer {
        McpServer::Stdio(
            McpServerStdio::new(SERVER_NAME, self.bridge.as_str())
                .args(vec![
                    SOCKET_FLAG.to_owned(),
                    self.socket.as_str().to_owned(),
                ])
                .env(vec![EnvVariable::new(EDGE_TOKEN_ENV, token.reveal())]),
        )
    }
}
