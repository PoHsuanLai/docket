//! The requests we send an agent, as lines. Every payload is a schema-crate type.

use super::spawn::{SessionMeta, SignIn};
use crate::out;
use agent_client_protocol_schema::ProtocolVersion;
use agent_client_protocol_schema::rpc::RequestId;
use agent_client_protocol_schema::v1::{
    AGENT_METHOD_NAMES, AuthenticateRequest, CancelNotification, ClientCapabilities, ContentBlock,
    FileSystemCapabilities, Implementation, InitializeRequest, McpServer, NewSessionRequest,
    PromptRequest, SessionId,
};
use docket_core::AbsPath;
use std::path::PathBuf;

/// Request numbers for one connection.
#[derive(Debug, Clone, Copy, Default)]
pub struct Ids(i64);

impl Ids {
    /// The next number.
    pub fn next(&mut self) -> RequestId {
        self.0 += 1;
        RequestId::Number(self.0)
    }
}

/// What we offer the agent: file reads and writes through us, and a terminal when `terminal`
/// says the sandbox is there. Nothing else (no elicitation, no auth terminal).
pub fn capabilities(terminal: ClientCapabilities) -> ClientCapabilities {
    terminal.fs(FileSystemCapabilities::new()
        .read_text_file(true)
        .write_text_file(true))
}

/// `initialize`.
pub fn initialize(id: &RequestId, caps: ClientCapabilities) -> String {
    let request = InitializeRequest::new(ProtocolVersion::V1)
        .client_capabilities(caps)
        .client_info(Implementation::new("docket", env!("CARGO_PKG_VERSION")));
    out::ask(id, AGENT_METHOD_NAMES.initialize, &request)
}

/// `authenticate` with the way the person chose. The agent signs itself in from the login it
/// already holds; nothing secret passes through here.
pub fn authenticate(id: &RequestId, method: &SignIn) -> String {
    let request = AuthenticateRequest::new(method.as_str().to_owned());
    out::ask(id, AGENT_METHOD_NAMES.authenticate, &request)
}

/// `session/new` in `cwd`, offering `servers`: the one tool edge of this session when there is
/// one, else none. Nothing else is imported; the agent's other tools are its own.
/// `meta` is the launcher's extra `_meta`, if any.
pub fn session_new(
    id: &RequestId,
    cwd: &AbsPath,
    servers: Vec<McpServer>,
    meta: Option<SessionMeta>,
) -> String {
    let request = NewSessionRequest::new(PathBuf::from(cwd.as_str()))
        .mcp_servers(servers)
        .meta(meta);
    out::ask(id, AGENT_METHOD_NAMES.session_new, &request)
}

/// `session/prompt` with the person's words and nothing else.
pub fn prompt(id: &RequestId, session: &SessionId, text: &str) -> String {
    let request = PromptRequest::new(session.clone(), vec![ContentBlock::from(text.to_owned())]);
    out::ask(id, AGENT_METHOD_NAMES.session_prompt, &request)
}

/// `session/cancel`.
pub fn cancel(session: &SessionId) -> String {
    out::notify(
        AGENT_METHOD_NAMES.session_cancel,
        &CancelNotification::new(session.clone()),
    )
}
