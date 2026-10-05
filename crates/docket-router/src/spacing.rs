//! Where a call's targets live. An entity id names no Space (`prov::EntityId` is porter's), so the
//! router asks a resolver: today the shadow index, which holds the Space scope each app pushed for
//! each thing it indexed. A target the resolver does not know is taken to be in the session's own
//! Space (the call is the app's to refuse); one scoped to another Space makes the call
//! cross-Space, which Cedar sends to the person (and denies an MCP client).

use crate::state::RouterState;
use docket_core::TargetKind;
use policy_point::SpaceRelation;
use prov::{EntityId, SpaceId, SpaceScope};

/// Which Spaces an entity belongs to, as far as the router knows.
pub trait SpaceOf {
    /// The scope of `entity`; none when nothing is known about it.
    fn scope_of(&self, entity: &EntityId) -> Option<SpaceScope>;
}

impl SpaceOf for RouterState {
    fn scope_of(&self, entity: &EntityId) -> Option<SpaceScope> {
        self.shadow
            .get(&entity.app)
            .and_then(|app| app.get(&(entity.kind.clone(), entity.key.clone())))
            .map(|entry| entry.space.clone())
    }
}

/// The relation of a call's targets to the Space of the session making it: unbound for an action
/// on nothing, `Other` when any target is scoped to a Space that is not the session's (a scope of
/// `Any` is the session's too), else `Same`.
pub fn relation_of(
    resolver: &impl SpaceOf,
    session: &SpaceId,
    on: &TargetKind,
    targets: &[EntityId],
) -> SpaceRelation {
    if matches!(on, TargetKind::Nothing) {
        return SpaceRelation::Unbound;
    }
    let elsewhere = targets.iter().any(
        |t| matches!(resolver.scope_of(t), Some(SpaceScope::Only(space)) if space != *session),
    );
    match elsewhere {
        true => SpaceRelation::Other,
        false => SpaceRelation::Same,
    }
}
