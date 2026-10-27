//! A session's id as the protocol names it.

use agent_client_protocol::schema::v1::SessionId;

/// A session's id as the protocol names it.
pub fn wire_id(session: &prov::SessionId) -> SessionId {
    SessionId::new(session.as_str())
}
