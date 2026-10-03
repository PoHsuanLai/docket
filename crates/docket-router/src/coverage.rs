//! Whether a call is inside the task policy. `docket_core::covers` judges the call from the
//! manifest's declaration (its effect, its cap and where each argument goes) and the labels; the
//! clock's expiry is the router's.

use docket_core::ActionMatch;
use docket_core::{ActionDecl, ArgLabels, CallRequest, Coverage, TaskPolicy, Widening, covers};
use prov::UnixSeconds;

/// The coverage of `call` under `policy` at `now`. No policy covers nothing.
pub(crate) fn coverage(
    policy: Option<&TaskPolicy>,
    decl: &ActionDecl,
    call: &CallRequest,
    labels: &ArgLabels,
    now: UnixSeconds,
) -> Coverage {
    match policy {
        None => Coverage::Outside(Widening::Action(ActionMatch::One(call.action.clone()))),
        Some(policy) if policy.expires < now => Coverage::Outside(Widening::Expiry),
        Some(policy) => covers(policy, decl, call, labels),
    }
}
