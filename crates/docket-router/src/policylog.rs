//! The log line for a call the policy point could not evaluate. The ruling is already the
//! stricter reading (see `policy_point::Pdp::decide_checked`); this says why, with the task.

use policy_point::Ruled;
use prov::TaskId;

/// Logs each evaluation fault of `ruled` with the task it was for.
pub(crate) fn log_faults(ruled: &Ruled, task: &TaskId) {
    for fault in &ruled.faults {
        tracing::warn!(
            %task,
            query = fault.query,
            cause = %fault.cause,
            "policy point could not evaluate; the ruling is the stricter reading"
        );
    }
}
