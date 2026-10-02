//! Whether a call is inside the task policy. `docket_core::covers` judges what a call and its
//! labels show; the effect an action has is the manifest's, so the ceiling and an app's cap
//! are checked here before it is asked.

use docket_core::{
    ActionDecl, ActionMatch, ArgLabels, CallRequest, Coverage, TaskPolicy, Widening, covers,
};
use prov::UnixSeconds;

/// The coverage of `call` under `policy` at `now`. No policy covers nothing.
pub(crate) fn coverage(
    policy: Option<&TaskPolicy>,
    decl: &ActionDecl,
    call: &CallRequest,
    labels: &ArgLabels,
    now: UnixSeconds,
) -> Coverage {
    let outside = || Coverage::Outside(Widening::Action(ActionMatch::One(call.action.clone())));
    let Some(policy) = policy else {
        return outside();
    };
    let named = policy.actions.iter().any(|m| match m {
        ActionMatch::One(a) => *a == call.action,
        ActionMatch::AppUpTo(app, cap) => *app == call.action.app && decl.effect <= *cap,
    });
    if policy.expires < now {
        Coverage::Outside(Widening::Expiry)
    } else if decl.effect > policy.ceiling {
        Coverage::Outside(Widening::Ceiling(decl.effect))
    } else if !named {
        outside()
    } else {
        covers(policy, call, labels)
    }
}
