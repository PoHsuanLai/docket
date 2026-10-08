//! A permission request for a call to our own tool edge, recognised. An agent that asks its
//! client before every tool of its own asks before the desktop's tools too, and the call itself
//! is gated again when it reaches the edge: two sheets for one action. The request says which
//! server and tool it is about in `_meta.mcp` (what the agent's runtime dispatches on), so a
//! request that names our server and a tool the edge offered is answered "once" without asking
//! the router: allowing it only lets the call arrive at the edge, which rules on it.
//!
//! Anything else is not recognised and goes the usual way: another server, a tool we did not
//! offer, no `_meta`, a kind that is not `other` or `fetch`, or a request that touches paths.
//! Listing our own server's resources (`rawInput` is exactly `{"ServerName": "quire"}`, no
//! `_meta.mcp`) is recognised too: the edge serves tools only, so the list reveals nothing a
//! `tools/list` does not.

use super::edge::SERVER_NAME;
use agent_client_protocol_schema::v1::{ToolCallUpdate, ToolKind};
use serde_json::Value;

/// What a recognised request is about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnEdge {
    /// A call to the named tool of our server; only if the edge offered it.
    Tool(String),
    /// A listing of our server's resources.
    Resources,
}

fn mcp(update: &ToolCallUpdate) -> Option<&Value> {
    update.meta.as_ref()?.get("mcp")
}

/// The request's claim to be about our edge, before anyone checks the tool is offered.
pub fn claim(update: &ToolCallUpdate) -> Option<OwnEdge> {
    let fields = &update.fields;
    if !matches!(fields.kind, Some(ToolKind::Other | ToolKind::Fetch))
        || fields.locations.as_ref().is_some_and(|l| !l.is_empty())
    {
        return None;
    }
    let Some(mcp) = mcp(update) else {
        let raw = fields.raw_input.as_ref()?.as_object()?;
        let only_ours =
            raw.len() == 1 && raw.get("ServerName").and_then(Value::as_str) == Some(SERVER_NAME);
        return only_ours.then_some(OwnEdge::Resources);
    };
    let flagged = update
        .meta
        .as_ref()
        .and_then(|m| m.get("is_mcp_tool_call"))
        .is_none_or(|v| v == &Value::Bool(true));
    let ours = mcp.get("server").and_then(Value::as_str) == Some(SERVER_NAME);
    let tool = mcp.get("tool").and_then(Value::as_str)?;
    (flagged && ours).then(|| OwnEdge::Tool(tool.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn update(v: Value) -> ToolCallUpdate {
        serde_json::from_value(v).expect("update")
    }

    fn tool(server: &str, kind: &str, flag: Value) -> ToolCallUpdate {
        update(json!({
            "toolCallId": "t", "kind": kind, "rawInput": {"arguments": {}},
            "_meta": { "mcp": {"tool": "mail__x", "server": server}, "is_mcp_tool_call": flag }
        }))
    }

    #[test]
    fn our_server_and_a_plain_kind_is_claimed() {
        let want = Some(OwnEdge::Tool("mail__x".into()));
        assert_eq!(claim(&tool(SERVER_NAME, "other", json!(true))), want);
        assert_eq!(claim(&tool(SERVER_NAME, "fetch", json!(true))), want);
        assert_eq!(claim(&tool("elsewhere", "other", json!(true))), None);
        assert_eq!(claim(&tool(SERVER_NAME, "execute", json!(true))), None);
        assert_eq!(claim(&tool(SERVER_NAME, "other", json!(false))), None);
    }

    #[test]
    fn resources_of_our_server_only_with_nothing_else_asked() {
        let raw = |raw: Value| update(json!({"toolCallId": "t", "kind": "other", "rawInput": raw}));
        assert_eq!(
            claim(&raw(json!({"ServerName": SERVER_NAME}))),
            Some(OwnEdge::Resources)
        );
        assert_eq!(claim(&raw(json!({"ServerName": "x"}))), None);
        assert_eq!(
            claim(&raw(json!({"ServerName": SERVER_NAME, "Uri": "u"}))),
            None
        );
        assert_eq!(claim(&raw(json!({"command": "ls"}))), None);
    }
}
