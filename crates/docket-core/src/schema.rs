//! The JSON Schema of an action's arguments: the one source for the planner's grammar and for
//! MCP. The planner supplies entity arguments as the `{app, kind, key}` object of an `EntityId`
//! and handles as `{ "handle": n }`; the router labels everything it receives.

use crate::manifest::ActionDecl;
use serde::{Deserialize, Serialize};

/// A JSON Schema document for one action's arguments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ToolSchema(pub serde_json::Value);

/// The schema of `action`'s arguments (an object with one property per parameter, the required
/// ones listed, no additional properties).
pub fn tool_schema(action: &ActionDecl) -> ToolSchema {
    let _ = action;
    todo!(
        "tool_schema: one property per ParamDecl by ParamType; Required listed; additionalProperties false; pinned by a snapshot test"
    )
}
