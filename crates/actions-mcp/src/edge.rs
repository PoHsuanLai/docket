//! The server: `rmcp` over stdio or a Unix socket, with the router behind it.

use docket_client::{Intents, Transport};
use prov::ClientName;
use rmcp::ServerHandler;
use rmcp::model::ServerConfig;

/// An MCP server over the router, speaking as one client.
#[derive(Debug)]
pub struct McpEdge<T: Transport> {
    intents: Intents<T>,
    client: ClientName,
}

impl<T: Transport> McpEdge<T> {
    /// Serves the tools of the registry to `client`, calling the router through `intents`.
    pub fn new(intents: Intents<T>, client: ClientName) -> Self {
        Self { intents, client }
    }

    /// One tool call: finds the action by tool name, reads the arguments by their declared
    /// types, labels every one `Untrusted` from this client, performs it as `Actor::Mcp`, and
    /// answers the outcome or the coarse refusal. A confirmation the router asks for is never
    /// answered here: the person answers it on the sheet.
    pub async fn call(
        &self,
        tool: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, crate::McpFault> {
        let _ = (&self.intents, &self.client, tool, arguments);
        todo!(
            "McpEdge::call: tool name -> ActionRef, JSON arguments -> Args by ParamType with mcp_label on each, Intents::perform, Outcome or CallRefusal -> a tool result; an MCP actor never gets a review: Allow for Read, Ask for the rest"
        )
    }
}

impl<T: Transport + 'static> ServerHandler for McpEdge<T> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::default()
    }
}
