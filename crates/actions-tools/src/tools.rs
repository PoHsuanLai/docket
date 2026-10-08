//! The tools a registry offers to MCP clients.

use crate::args::TARGET_KEY;
use crate::fault::McpFault;
use docket_core::{
    ActionDecl, AgentReach, TargetKind, ToolSchema, ValidManifest, Visibility, action_prefix,
    tool_schema,
};
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
        // A command can delete as well as reach out: the strictest hint.
        Effect::Destructive | Effect::Execute => ToolHints::Destructive,
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

/// The actions offered to clients: only `AgentReach::Offered`, of apps that are not host-only.
/// Hidden ones are dropped, and so are the ones that ask every time (a client cannot answer a
/// sheet).
pub fn offered(registry: &[ValidManifest]) -> Vec<(&ValidManifest, &ActionDecl)> {
    registry
        .iter()
        .filter(|m| m.manifest().visibility == Visibility::Everyone)
        .flat_map(|m| m.manifest().actions.iter().map(move |a| (m, a)))
        .filter(|(_, a)| a.reach == AgentReach::Offered)
        .collect()
}

/// The schema of an action's arguments plus the `target` key its `on` asks for: one entity, a
/// list of entities or a list of files. A live text field has no key (a client cannot name it).
fn schema_of(action: &ActionDecl) -> ToolSchema {
    let ToolSchema(mut schema) = tool_schema(action);
    let entity = |kind: &prov::EntityKind| {
        serde_json::json!({
            "type": "object",
            "properties": {
                "app": { "type": "string" },
                "kind": { "const": kind.as_str() },
                "key": { "type": "string" },
            },
            "required": ["app", "kind", "key"],
            "additionalProperties": false,
        })
    };
    let target = match &action.on {
        TargetKind::Nothing | TargetKind::Text => None,
        TargetKind::One(kind) => Some(entity(kind)),
        TargetKind::Many(kind) => {
            Some(serde_json::json!({ "type": "array", "minItems": 1, "items": entity(kind) }))
        }
        TargetKind::Files => Some(
            serde_json::json!({ "type": "array", "minItems": 1, "items": { "type": "string" } }),
        ),
    };
    if let (Some(target), Some(object)) = (target, schema.as_object_mut()) {
        if let Some(properties) = object.get_mut("properties").and_then(|p| p.as_object_mut()) {
            properties.insert(TARGET_KEY.to_owned(), target);
        }
        if let Some(required) = object.get_mut("required").and_then(|r| r.as_array_mut()) {
            required.push(serde_json::json!(TARGET_KEY));
        }
    }
    ToolSchema(schema)
}

/// The tool of one offered action, with the action's label as its description. An action whose
/// name would be too long gets none.
pub fn tool_of(manifest: &ValidManifest, action: &ActionDecl) -> Option<(McpTool, String)> {
    let name = tool_name(&manifest.manifest().app, action).ok()?;
    let tool = McpTool {
        name,
        schema: schema_of(action),
        annotations: hints_of(action.effect),
    };
    Some((tool, action.label.as_str().to_owned()))
}

/// The tools of a registry, in registry order. An action whose name would be too long gets no
/// tool. The `target` key is part of each schema where the action acts on something.
pub fn tools(registry: &[ValidManifest]) -> Vec<McpTool> {
    offered(registry)
        .into_iter()
        .filter_map(|(m, a)| tool_of(m, a).map(|(tool, _)| tool))
        .collect()
}

/// The offered action a tool name stands for, if there is one.
pub fn find<'a>(
    registry: &'a [ValidManifest],
    tool: &str,
) -> Option<(&'a ValidManifest, &'a ActionDecl)> {
    offered(registry)
        .into_iter()
        .find(|(m, a)| tool_name(&m.manifest().app, a).is_ok_and(|n| n.as_str() == tool))
}
