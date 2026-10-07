//! The messages we write, as lines. Every payload is a schema-crate type, so a field the protocol
//! does not have cannot be added by accident.

use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::Error;
use serde::Serialize;
use serde_json::{Value, json};

fn value(of: &impl Serialize) -> Value {
    serde_json::to_value(of).unwrap_or(Value::Null)
}

/// A session's id as the protocol names it.
pub fn wire_id(session: &prov::SessionId) -> agent_client_protocol_schema::v1::SessionId {
    agent_client_protocol_schema::v1::SessionId::new(session.as_str())
}

/// A success reply.
pub fn reply(id: &RequestId, result: &impl Serialize) -> String {
    json!({"jsonrpc": "2.0", "id": value(id), "result": value(result)}).to_string()
}

/// An error reply.
pub fn failure(id: &RequestId, error: &Error) -> String {
    json!({"jsonrpc": "2.0", "id": value(id), "error": value(error)}).to_string()
}

/// A notification.
pub fn notify(method: &str, params: &impl Serialize) -> String {
    json!({"jsonrpc": "2.0", "method": method, "params": value(params)}).to_string()
}

/// A request to the editor.
pub fn ask(id: &RequestId, method: &str, params: &impl Serialize) -> String {
    json!({"jsonrpc": "2.0", "id": value(id), "method": method, "params": value(params)})
        .to_string()
}
