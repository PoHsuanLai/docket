//! Standing consent for one call: the person's grants, looked up per data class the action
//! touches, and what a tainted session does to an `Always`.

use crate::session::Taint;
use docket_core::{ActionDecl, ActionGrant, ActionGrantKey, GrantCaller, GrantTarget};
use porter_core::consent::{GrantScope, Usage, Verdict, decide};
use porter_core::{DataClass, GrantId};
use prov::{SpaceId, SpaceScope};

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
/// session that read untrusted content cannot lean on an `Always` for anything that writes: it
/// asks once more. Reading stays free, or one mail read would put a question on every read after.
pub(crate) fn consent_for(
    grants: &[ActionGrant],
    caller: &GrantCaller,
    decl: &ActionDecl,
    app: &porter_core::AppName,
    space: &SpaceId,
    usage: Usage,
    taint: Taint,
) -> Verdict {
    let per_class: Vec<Verdict> = decl
        .classes
        .iter()
        .map(|class| {
            keys(caller, decl, app, *class, usage, space)
                .iter()
                .map(|k| decide(grants, k))
                .find(|v| !matches!(v, Verdict::Ask))
                .unwrap_or(Verdict::Ask)
        })
        .collect();
    let granted = per_class.iter().find_map(|v| match v {
        Verdict::Granted { grant, scope } => Some((grant.clone(), *scope)),
        Verdict::Denied | Verdict::Ask => None,
    });
    if per_class.iter().any(|v| matches!(v, Verdict::Denied)) {
        Verdict::Denied
    } else if per_class.iter().any(|v| matches!(v, Verdict::Ask)) {
        Verdict::Ask
    } else {
        match granted {
            Some((_, GrantScope::Always)) if taint == Taint::Tainted => Verdict::Ask,
            Some((grant, scope)) => Verdict::Granted { grant, scope },
            None => Verdict::Granted {
                grant: GrantId::parse("g-none").expect("`g-none` is a valid grant id"),
                scope: GrantScope::Once,
            },
        }
    }
}
