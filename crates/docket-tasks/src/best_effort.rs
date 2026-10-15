//! Calls whose failure the companion survives but the operator should be able to read. A failed
//! episode write or a session that would not close does not stop the task; it leaves one line
//! in the journal naming the call, the session or task it was for, and the cause.

use std::fmt::Display;

/// Logs `result`'s failure, if any, as `call` for `who` (a session or task id); the value of a
/// success is dropped. Nothing of a message body is logged, only the cause the link names.
pub(crate) fn best_effort<T, E: Display>(
    call: &'static str,
    who: &impl Display,
    result: Result<T, E>,
) {
    if let Err(cause) = result {
        tracing::warn!(call, %who, %cause, "a best-effort call failed");
    }
}
