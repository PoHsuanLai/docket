//! Standing consent for one call: the person's grants, looked up per data class the action
//! touches. What a tainted session does to an `Always` is the policy's (`tainted-always-asks`).

use crate::companion_names::LIMIT_ADD;
use docket_manifest::ActionDecl;
use docket_permissions_core::{
    ActionGrant, ActionGrantKey, COMPANION_APP, GrantCaller, GrantTarget,
};
use docket_session::Taint;
use porter_core::consent::{GrantScope, Usage, Verdict, decide};
use porter_core::{DataClass, GrantId};
use prov::{ActionName, SpaceId, SpaceScope};

/// Whether a call is the assistant's own `companion.limit.add`: the one action that needs no
/// first-use question, because it can only make things stricter.
fn is_limit_add(app: &porter_core::AppName, name: &ActionName) -> bool {
    app.as_str() == COMPANION_APP && name.as_str() == LIMIT_ADD
}

/// The grants to try for one class, narrowest first: this action in this Space, the whole app
/// in this Space, then the same across Spaces.
fn keys(
    caller: &GrantCaller,
    decl: &ActionDecl,
    app: &porter_core::AppName,
    class: DataClass,
    usage: Usage,
    space: &SpaceId,
) -> Vec<ActionGrantKey> {
    let key = |target: GrantTarget, space: SpaceScope| ActionGrantKey {
        caller: caller.clone(),
        owner: app.clone(),
        target,
        class,
        usage,
        space,
    };
    let only = SpaceScope::Only(space.clone());
    vec![
        key(GrantTarget::Action(decl.name.clone()), only.clone()),
        key(GrantTarget::App, only),
        key(GrantTarget::Action(decl.name.clone()), SpaceScope::Any),
        key(GrantTarget::App, SpaceScope::Any),
    ]
}

/// What standing consent says about `decl` for `caller` in `space`: denied if any class is, to
/// ask if any is not yet granted, else granted. An action that touches no class needs none. A
/// tainted session's `Always` stays an `Always` here: the policy asks once more.
pub(crate) fn consent_for(
    grants: &[ActionGrant],
    caller: &GrantCaller,
    decl: &ActionDecl,
    app: &porter_core::AppName,
    space: &SpaceId,
    usage: Usage,
) -> Verdict {
    // A terminal has no class grants (a read from it is allowed and everything else asks by the
    // policy's own rule), but a denial the person recorded for it still stands.
    let unasked = match caller {
        GrantCaller::Cli => Verdict::Granted {
            grant: GrantId::parse("g-none").expect("`g-none` is a valid grant id"),
            scope: GrantScope::Once,
        },
        // Launching an external agent in a directory is the person's own act: what it does there
        // is judged by the rest of the gate and by standing grants, not by a class question on
        // every first use. A recorded denial still stands, and a session that read untrusted
        // content asks again for anything that writes (the policy's `tainted-always-asks`). Only for the pseudo-app: the
        // person launched the agent in a directory, not into their mail, so an action of another
        // app that the agent calls over its tool edge is asked about like a planner's.
        GrantCaller::AcpAgent(_) | GrantCaller::AcpHelper { .. }
            if app.as_str() == docket_permissions_core::ACP_AGENT_APP =>
        {
            Verdict::Granted {
                grant: GrantId::parse("g-none").expect("`g-none` is a valid grant id"),
                scope: GrantScope::Always,
            }
        }
        // The assistant tightening on the person's word needs no question: it can only make
        // the next call ask or refuse.
        GrantCaller::Companion if is_limit_add(app, &decl.name) => Verdict::Granted {
            grant: GrantId::parse("g-none").expect("`g-none` is a valid grant id"),
            scope: GrantScope::Always,
        },
        _ => Verdict::Ask,
    };
    let per_class: Vec<Verdict> = decl
        .classes
        .iter()
        .map(|class| {
            keys(caller, decl, app, *class, usage, space)
                .iter()
                .map(|k| decide(grants, k))
                .find(|v| !matches!(v, Verdict::Ask))
                .unwrap_or_else(|| unasked.clone())
        })
        .collect();
    let granted = per_class.iter().find_map(|v| match v {
        Verdict::Granted { grant, scope } => Some((grant.clone(), scope.clone())),
        Verdict::Denied | Verdict::Ask => None,
    });
    if per_class.iter().any(|v| matches!(v, Verdict::Denied)) {
        Verdict::Denied
    } else if per_class.iter().any(|v| matches!(v, Verdict::Ask)) {
        Verdict::Ask
    } else {
        match granted {
            Some((grant, scope)) => Verdict::Granted { grant, scope },
            None => Verdict::Granted {
                grant: GrantId::parse("g-none").expect("`g-none` is a valid grant id"),
                scope: GrantScope::Once,
            },
        }
    }
}

/// A helper's call that is inside the reach the person set on its card: the card is a standing
/// grant for it, so a first-use question is already answered. As with every standing grant, a
/// session that read untrusted content asks once more before it writes (`taint`: the question
/// stays a question). A denial, a grant, or a call outside the reach is left as it is.
pub(crate) fn within_reach(consent: Verdict, inside: bool, taint: Taint) -> Verdict {
    match (consent, inside, taint) {
        (Verdict::Ask, true, Taint::Clean) => Verdict::Granted {
            grant: GrantId::parse("g-none").expect("`g-none` is a valid grant id"),
            scope: GrantScope::Always,
        },
        (other, _, _) => other,
    }
}
