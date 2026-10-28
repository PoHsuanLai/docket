//! Which calls a caller's role may make at all, before any policy is asked.

use crate::registry::Registry;
use docket_manifest::{ActionDecl, is_offered};
use docket_permissions_core::is_agent_action;
use docket_vocab::{ActionRef, CallerRole};

/// Whether a caller in `role` may make a call of `action`. The pseudo-app belongs to the host of
/// an external agent alone; that host makes it no call of another app but the ones the agent's
/// edge offers (`AgentReach::Offered` of an app that is not host-only), so a hostile agent cannot
/// reach an action nobody offered it by naming it.
pub(crate) fn may_make(
    registry: &Registry,
    role: CallerRole,
    action: &ActionRef,
    decl: &ActionDecl,
) -> bool {
    // Giving a job to a helper and proposing one are the companion's planners' alone.
    if crate::companion_names::is_helper_action(action) {
        return role == CallerRole::Companion;
    }
    match (role == CallerRole::AcpAgent, is_agent_action(action)) {
        (_, true) => role == CallerRole::AcpAgent,
        (false, false) => true,
        (true, false) => registry
            .get(&action.app)
            .is_some_and(|m| is_offered(m, decl)),
    }
}
