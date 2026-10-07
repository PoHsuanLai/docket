//! The line transport and the messages that cross it. ACP over stdio is one JSON-RPC message per
//! line; `Wire` is that and nothing more, so the lib names no runtime: the binary wraps stdin and
//! stdout, a test wraps a pair of in-memory channels.

use agent_client_protocol_schema::rpc::RequestId;
use serde_json::Value;
use std::future::Future;

/// Why a line could not be written: the peer is gone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("the peer closed the connection")]
pub struct WireClosed;

/// One line in, one line out. Reads must be cancel-safe: the server drops a pending read when an
/// event wins the race, and a line must not be lost.
pub trait Wire: Send {
    /// The next line, without its newline; `None` when the peer has closed.
    fn read_line(&mut self) -> impl Future<Output = Option<String>> + Send;

    /// Writes one message as one line.
    fn write_line(&mut self, line: String) -> impl Future<Output = Result<(), WireClosed>> + Send;
}

/// A message from the editor, sorted by what it is.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    /// A call that wants an answer.
    Request {
        /// To answer with.
        id: RequestId,
        /// The method.
        method: String,
        /// Its parameters (`null` when absent).
        params: Value,
    },
    /// A call that does not.
    Notification {
        /// The method.
        method: String,
        /// Its parameters (`null` when absent).
        params: Value,
    },
    /// The answer to something we asked.
    Reply {
        /// What we asked.
        id: RequestId,
        /// The result, or the error object.
        outcome: Result<Value, Value>,
    },
}

/// A line that is not a JSON-RPC 2.0 message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("not a JSON-RPC 2.0 message")]
pub struct NotJsonRpc;

impl Incoming {
    /// Reads one line.
    pub fn parse(line: &str) -> Result<Self, NotJsonRpc> {
        let Ok(Value::Object(mut body)) = serde_json::from_str::<Value>(line) else {
            return Err(NotJsonRpc);
        };
        if body.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            return Err(NotJsonRpc);
        }
        let id = body
            .remove("id")
            .map(serde_json::from_value::<RequestId>)
            .transpose()
            .map_err(|_| NotJsonRpc)?;
        let params = body.remove("params").unwrap_or(Value::Null);
        match (body.remove("method"), id) {
            (Some(Value::String(method)), Some(id)) => Ok(Incoming::Request { id, method, params }),
            (Some(Value::String(method)), None) => Ok(Incoming::Notification { method, params }),
            (Some(_), _) => Err(NotJsonRpc),
            (None, Some(id)) => match (body.remove("result"), body.remove("error")) {
                (Some(result), None) => Ok(Incoming::Reply {
                    id,
                    outcome: Ok(result),
                }),
                (None, Some(error)) => Ok(Incoming::Reply {
                    id,
                    outcome: Err(error),
                }),
                _ => Err(NotJsonRpc),
            },
            (None, None) => Err(NotJsonRpc),
        }
    }
}
