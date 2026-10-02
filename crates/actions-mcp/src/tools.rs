//! The tools a registry offers to MCP clients.

use docket_core::{ActionDecl, AgentReach, ToolSchema, ValidManifest, action_prefix, tool_schema};
use prov::Effect;
use serde::{Deserialize, Serialize};
use std::fmt;

/// An MCP tool name: `<app slug>__<action with dots as underscores>`, at most 64 characters of
/// `[A-Za-z0-9_-]`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct McpToolName(String);

impl McpToolName {
    /// The name's text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for McpToolName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why an action has no tool name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum McpFault {
    /// The name would be longer than 64 characters.
    #[error("tool name longer than 64 characters")]
    TooLong,
}

/// How an MCP client is told what a tool does (the protocol's hints, which are advice and not
/// guarantees: the router still gates every call).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolHints {
    /// Changes nothing.
    ReadOnly,
    /// Changes something that can be taken back.
    Undoable,
    /// Reaches outside the machine.
    OpenWorld,
    /// Removes something for good.
    Destructive,
}

/// The hint for an effect.
pub fn hints_of(effect: Effect) -> ToolHints {
    match effect {
        Effect::Read => ToolHints::ReadOnly,
        Effect::UndoableWrite => ToolHints::Undoable,
        Effect::Outbound => ToolHints::OpenWorld,
        Effect::Destructive => ToolHints::Destructive,
    }
}

/// One tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpTool {
    /// Its name.
    pub name: McpToolName,
    /// The schema of its arguments.
    pub schema: ToolSchema,
    /// What it does, as a hint.
    pub annotations: ToolHints,
}

/// The tool name of an action of an app.
pub fn tool_name(app: &porter_core::AppName, action: &ActionDecl) -> Result<McpToolName, McpFault> {
    let name = format!(
        "{}__{}",
        action_prefix(app),
        action.name.as_str().replace('.', "_")
    );
    if name.len() > 64 {
        Err(McpFault::TooLong)
    } else {
        Ok(McpToolName(name))
    }
}

/// The actions offered to clients: only `AgentReach::Offered`. Hidden ones are dropped, and so
/// are the ones that ask every time (a client cannot answer a sheet).
pub fn offered(registry: &[ValidManifest]) -> Vec<(&ValidManifest, &ActionDecl)> {
    registry
        .iter()
        .flat_map(|m| m.manifest().actions.iter().map(move |a| (m, a)))
        .filter(|(_, a)| a.reach == AgentReach::Offered)
        .collect()
}

/// The tools of a registry, in registry order. An action whose name would be too long gets no
/// tool.
pub fn tools(registry: &[ValidManifest]) -> Vec<McpTool> {
    offered(registry)
        .into_iter()
        .filter_map(|(m, a)| {
            let name = tool_name(&m.manifest().app, a).ok()?;
            Some(McpTool {
                name,
                schema: tool_schema(a),
                annotations: hints_of(a.effect),
            })
        })
        .collect()
}
