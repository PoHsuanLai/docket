//! The requests the scripted agent makes, as JSON: a permission request, a write, a read.

/// The tool-call fields a permission request carries.
pub fn tool(kind: &str, title: &str, paths: &[&str], raw: serde_json::Value) -> serde_json::Value {
    let locations: Vec<_> = paths
        .iter()
        .map(|p| serde_json::json!({"path": p}))
        .collect();
    serde_json::json!({
        "sessionId": super::agent::AGENT_SESSION,
        "toolCall": {
            "toolCallId": "tc-1", "kind": kind, "title": title,
            "locations": locations, "rawInput": raw
        },
        "options": [
            {"optionId": "a-always", "name": "Always", "kind": "allow_always"},
            {"optionId": "a-once", "name": "Once", "kind": "allow_once"},
            {"optionId": "r-once", "name": "No", "kind": "reject_once"}
        ]
    })
}

pub use super::agent::read;

pub fn write(path: &str, content: &str) -> serde_json::Value {
    serde_json::json!({"sessionId": super::agent::AGENT_SESSION, "path": path, "content": content})
}

pub fn selected(reply: &Result<serde_json::Value, serde_json::Value>) -> Option<String> {
    let value = reply.as_ref().ok()?;
    value["outcome"]["optionId"].as_str().map(str::to_owned)
}

/// A permission request as agy sends it for a call to an MCP tool: `kind` other, and the
/// server and tool in `_meta.mcp`; `meta` is that object, or none to leave `_meta` out.
pub fn mcp_request(
    kind: &str,
    meta: Option<serde_json::Value>,
    raw: serde_json::Value,
    options: serde_json::Value,
) -> serde_json::Value {
    let mut call = serde_json::json!({
        "toolCallId": "tc-1", "kind": kind, "status": "pending",
        "title": "quire_mail__mail_thread_read", "content": [], "rawInput": raw
    });
    if let Some(mcp) = meta {
        call["_meta"] = serde_json::json!({"mcp": mcp, "is_mcp_tool_call": true});
    }
    serde_json::json!({"sessionId": super::agent::AGENT_SESSION, "toolCall": call, "options": options})
}

/// The three options agy offers.
pub fn agy_options() -> serde_json::Value {
    serde_json::json!([
        {"optionId": "a-always", "name": "Allow Always", "kind": "allow_always"},
        {"optionId": "a-once", "name": "Allow", "kind": "allow_once"},
        {"optionId": "r-once", "name": "Deny", "kind": "reject_once"}
    ])
}
