//! The bridge an external ACP agent starts: an MCP server with no authority of its own. It is
//! given a unix socket (a command-line argument) and a token (its environment), forwards
//! `tools/list` and `tools/call` over the socket, and returns what the host answers. The host
//! made the socket for one session and makes each call in that session; this process cannot name
//! another, because the lines it sends have nowhere to write one (`actions_tools::EdgeRequest`).
//! It does not touch the bus, the configuration or the settings.

use crate::edge::{failed, listed, rmcp_tool};
use actions_tools::{
    EDGE_LINE_MAX, EDGE_TOKEN_ENV, EdgeOp, EdgeReply, EdgeRequest, EdgeToken, McpFault,
};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use serde_json::Value as Json;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;

/// An MCP server whose every request is one line to the host's socket.
#[derive(Debug, Clone)]
pub struct BoundEdge {
    socket: PathBuf,
    token: EdgeToken,
}

/// Why the token could not be read from the environment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("{EDGE_TOKEN_ENV} is missing or not a session token")]
pub struct NoToken;

impl BoundEdge {
    /// A bridge to `socket` speaking with `token`.
    pub fn new(socket: PathBuf, token: EdgeToken) -> Self {
        Self { socket, token }
    }

    /// A bridge to `socket` with the token the environment names.
    pub fn from_env(
        socket: PathBuf,
        env: &impl Fn(&str) -> Option<String>,
    ) -> Result<Self, NoToken> {
        let token = env(EDGE_TOKEN_ENV)
            .and_then(|t| EdgeToken::parse(&t))
            .ok_or(NoToken)?;
        Ok(Self::new(socket, token))
    }

    /// One request and its reply. Any trouble with the socket is the router being unavailable.
    async fn ask(&self, op: EdgeOp) -> Result<EdgeReply, McpFault> {
        let request = EdgeRequest {
            token: self.token.clone(),
            op,
        };
        let mut text = serde_json::to_string(&request).map_err(|_| McpFault::Unavailable)?;
        text.push('\n');
        let stream = UnixStream::connect(&self.socket)
            .await
            .map_err(|_| McpFault::Unavailable)?;
        let (read, mut write) = stream.into_split();
        write
            .write_all(text.as_bytes())
            .await
            .map_err(|_| McpFault::Unavailable)?;
        let mut reader = BufReader::new(read).take(EDGE_LINE_MAX as u64);
        let mut line = Vec::new();
        reader
            .read_until(b'\n', &mut line)
            .await
            .map_err(|_| McpFault::Unavailable)?;
        serde_json::from_slice(&line).map_err(|_| McpFault::Unavailable)
    }
}

impl ServerHandler for BoundEdge {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = match self.ask(EdgeOp::List).await {
            Ok(EdgeReply::Tools(tools)) => tools,
            Ok(_) | Err(_) => Vec::new(),
        };
        Ok(listed(
            tools
                .into_iter()
                .filter_map(|t| rmcp_tool(t.tool, t.description))
                .collect(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = request.arguments.map_or(Json::Null, Json::Object);
        let op = EdgeOp::Call {
            tool: request.name.to_string(),
            arguments,
        };
        let result = match self.ask(op).await {
            Ok(EdgeReply::Done(value)) => CallToolResult::success(vec![ContentBlock::json(value)?]),
            Ok(EdgeReply::Failed(fault)) | Err(fault) => failed(&fault),
            Ok(EdgeReply::Refused | EdgeReply::Tools(_)) => failed(&McpFault::Unavailable),
        };
        Ok(result.into())
    }
}
