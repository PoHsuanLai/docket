//! The thing a conversation names itself as in memory: the one home of its identity, so the
//! session log that writes under it and the router that reads about it name the same thing.

use porter_core::AppName;
use prov::{EntityId, EntityKey, EntityKind, SessionId};

/// The app whose things the sessions are.
const SESSION_APP: &str = "org.quire.Companion";

/// The kind of the thing a session names itself as.
pub const SESSION_KIND: &str = "companion.session";

/// The thing `session` is in memory (every entry of its log names it as its `Subject`, and what
/// else records about the conversation may name it too); none when an id does not fit a key.
pub fn session_thing(session: &SessionId) -> Option<EntityId> {
    Some(EntityId {
        app: AppName::parse(SESSION_APP).ok()?,
        kind: EntityKind::parse(SESSION_KIND).ok()?,
        key: EntityKey::parse(session.as_str()).ok()?,
    })
}
