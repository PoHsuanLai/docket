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
