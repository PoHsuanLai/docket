//! One tool call prepared and one tool list made: the steps every edge of an outside agent takes
//! the same way, so the MCP edge and the per-session edge of an external ACP agent keep only their
//! transport.

use crate::fault::ToolFault;
use crate::label::tool_label;
use crate::tools::{ToolSpec, find, offered, tool_of};
use docket_manifest::{ActionDecl, ArgsFault, ValidManifest, args_from_json};
use docket_vocab::{ActionRef, CallRequest, Origin};
use prov::ClientName;
use serde_json::Value as Json;

/// The reference of an action of a manifest.
fn ref_of(manifest: &ValidManifest, decl: &ActionDecl) -> ActionRef {
    ActionRef {
        app: manifest.manifest().app.clone(),
        name: decl.name.clone(),
    }
}

/// The tools an edge lists, each with its description, in registry order: the offered actions
/// that `on_menu` lets through. An edge with no menu of its own passes `|_| true`.
pub fn listing(
    registry: &[ValidManifest],
    on_menu: impl Fn(&ActionRef) -> bool,
) -> Vec<(ToolSpec, String)> {
    offered(registry)
        .into_iter()
        .filter(|(m, a)| on_menu(&ref_of(m, a)))
        .filter_map(|(m, a)| tool_of(m, a))
        .collect()
}

/// The call a tool call stands for: finds the offered action by tool name (one that is not on the
/// menu is unknown), reads `arguments` by their declared types, labels each one `Untrusted` from
/// `client` and makes the request as `Origin::Mcp`. An argument that does not fit its declaration
/// is refused here, before anything is asked of the router.
pub fn prepare(
    registry: &[ValidManifest],
    tool: &str,
    arguments: &Json,
    client: &ClientName,
    on_menu: impl Fn(&ActionRef) -> bool,
) -> Result<CallRequest, ToolFault> {
    let (manifest, decl) = find(registry, tool)
        .filter(|(m, a)| on_menu(&ref_of(m, a)))
        .ok_or(ToolFault::UnknownTool)?;
    let given = match arguments {
        Json::Null => None,
        Json::Object(map) => Some(map),
        _ => return Err(ArgsFault::NotAnObject.into()),
    };
    let (target, args) = args_from_json(decl, given, &tool_label(client))?;
    Ok(CallRequest {
        action: ref_of(manifest, decl),
        target,
        args,
        origin: Origin::Mcp,
    })
}
