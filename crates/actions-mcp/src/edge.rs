//! The server: `rmcp` over stdio or a Unix socket, with the router behind it.

use crate::access::McpAccess;
use crate::args::read_call;
use crate::fault::{ArgsFault, McpFault, McpRefusal};
use crate::label::mcp_label;
use crate::result::outcome_json;
use crate::tools::{McpTool, ToolHints, offered, tool_name, tool_of};
use docket_client::{Intents, Transport};
use docket_core::{ActionDecl, ActionRef, CallRequest, Origin, ValidManifest};
use prov::ClientName;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, ErrorData,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    ToolAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::{RoleServer, ServerHandler};
use serde_json::Value as Json;
use std::sync::Arc;

/// An MCP server over the router, speaking as one client. Off until `with_access(McpAccess::On)`.
#[derive(Debug)]
pub struct McpEdge<T: Transport> {
    intents: Intents<T>,
    client: ClientName,
    access: McpAccess,
}

impl<T: Transport> McpEdge<T> {
    /// Serves the tools of the registry to `client`, calling the router through `intents`. The
    /// edge is off: it lists nothing and refuses every call until it is switched on.
    pub fn new(intents: Intents<T>, client: ClientName) -> Self {
        Self {
            intents,
            client,
            access: McpAccess::default(),
        }
    }

    /// The same edge with `access` (the person's setting).
    pub fn with_access(self, access: McpAccess) -> Self {
        Self { access, ..self }
    }

    /// Whether the edge is on.
    pub fn access(&self) -> McpAccess {
        self.access
    }

    async fn registry(&self) -> Result<Vec<ValidManifest>, McpFault> {
        self.intents
            .manifests()
            .await
            .map_err(|_| McpFault::Unavailable)
    }

    /// The tools offered right now, each with its description: nothing while the edge is off.
    pub async fn listing(&self) -> Result<Vec<(McpTool, String)>, McpFault> {
        match self.access {
            McpAccess::Off => Ok(Vec::new()),
            McpAccess::On => {
                let registry = self.registry().await?;
                Ok(offered(&registry)
                    .into_iter()
                    .filter_map(|(m, a)| tool_of(m, a))
                    .collect())
            }
        }
    }

    /// One tool call: finds the action by tool name, reads the arguments by their declared
    /// types, labels every one `Untrusted` from this client, performs it as `Actor::Mcp`, and
    /// answers the outcome or the coarse refusal. A confirmation the router asks for is never
    /// answered here: the person answers it on the sheet. An argument that does not fit its
    /// declaration is refused before the router is asked anything.
    pub async fn call(&self, tool: &str, arguments: Json) -> Result<Json, McpFault> {
        if self.access == McpAccess::Off {
            return Err(McpFault::Off);
        }
        let registry = self.registry().await?;
        let (manifest, decl) = find(&registry, tool).ok_or(McpFault::UnknownTool)?;
        let given = match &arguments {
            Json::Null => None,
            Json::Object(map) => Some(map),
            _ => return Err(ArgsFault::NotAnObject.into()),
        };
        let (target, args) = read_call(decl, given, &mcp_label(&self.client))?;
        let call = CallRequest {
            action: ActionRef {
                app: manifest.manifest().app.clone(),
                name: decl.name.clone(),
            },
            target,
            args,
            origin: Origin::Mcp,
        };
        match self.intents.perform(call, None, None).await {
            Ok(Ok(outcome)) => Ok(outcome_json(&outcome)),
            Ok(Err(refusal)) => Err(McpRefusal::from(&refusal).into()),
            Err(_) => Err(McpFault::Unavailable),
        }
    }
}

fn find<'a>(
    registry: &'a [ValidManifest],
    tool: &str,
) -> Option<(&'a ValidManifest, &'a ActionDecl)> {
    offered(registry)
        .into_iter()
        .find(|(m, a)| tool_name(&m.manifest().app, a).is_ok_and(|n| n.as_str() == tool))
}

fn annotations(hints: ToolHints) -> ToolAnnotations {
    match hints {
        ToolHints::ReadOnly => ToolAnnotations::new().read_only(true),
        ToolHints::Undoable => ToolAnnotations::new().read_only(false).destructive(false),
        ToolHints::OpenWorld => ToolAnnotations::new()
            .read_only(false)
            .destructive(false)
            .open_world(true),
        ToolHints::Destructive => ToolAnnotations::new().read_only(false).destructive(true),
    }
}

fn rmcp_tool(tool: McpTool, description: String) -> Option<Tool> {
    let schema = tool.schema.0.as_object()?.clone();
    let mut listed = Tool::new(tool.name.to_string(), description, Arc::new(schema));
    listed.annotations = Some(annotations(tool.annotations));
    Some(listed)
}

fn failed(fault: &McpFault) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(fault.to_string())])
}

impl<T: Transport + 'static> ServerHandler for McpEdge<T> {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let listing = self
            .listing()
            .await
            .map_err(|fault| ErrorData::internal_error(fault.to_string(), None))?;
        Ok(ListToolsResult::with_all_items(
            listing
                .into_iter()
                .filter_map(|(tool, description)| rmcp_tool(tool, description))
                .collect(),
        ))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let arguments = request.arguments.map_or(Json::Null, Json::Object);
        let result = match self.call(&request.name, arguments).await {
            Ok(value) => CallToolResult::success(vec![ContentBlock::json(value)?]),
            Err(fault) => failed(&fault),
        };
        Ok(result.into())
    }
}
