//! The editor's permission request: an extra gate on top of ours. It offers `allow_once` and one
//! reject, never `allow_always` or `reject_always`; an answer that names anything else, an error,
//! or a cancel is a reject. An allow lets the call go on to our gate. It is not a grant, not a
//! receipt, and it answers no confirmation sheet.

use crate::calls;
use agent_client_protocol_schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SessionId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use docket_session::CallOpen;
use serde_json::Value;

/// The id of the one option that lets a call go on.
pub const ALLOW_ONCE: &str = "allow_once";
/// The id of the option that stops it.
pub const REJECT_ONCE: &str = "reject_once";

/// What the editor's person said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Go on, to our gate.
    Allow,
    /// Stop.
    Reject,
}

/// The options offered: two, always.
pub fn options() -> Vec<PermissionOption> {
    vec![
        PermissionOption::new(ALLOW_ONCE, "Allow once", PermissionOptionKind::AllowOnce),
        PermissionOption::new(REJECT_ONCE, "Reject", PermissionOptionKind::RejectOnce),
    ]
}

/// The request for `call`, naming it by its action and effect only.
pub fn request(session: &SessionId, call: &CallOpen) -> RequestPermissionRequest {
    let fields = ToolCallUpdateFields::new()
        .title(call.action.name.as_str().to_owned())
        .status(ToolCallStatus::Pending);
    let tool = ToolCallUpdate::new(calls::call_id(call.call), fields);
    RequestPermissionRequest::new(session.clone(), tool, options())
}

/// The verdict in an outcome: only a selection of `allow_once` allows.
pub fn verdict(outcome: &RequestPermissionOutcome) -> Verdict {
    match outcome {
        RequestPermissionOutcome::Selected(chosen) if &*chosen.option_id.0 == ALLOW_ONCE => {
            Verdict::Allow
        }
        _ => Verdict::Reject,
    }
}

/// The verdict in a reply to our request: anything unreadable is a reject.
pub fn verdict_of_reply(reply: Result<Value, Value>) -> Verdict {
    reply
        .ok()
        .and_then(|v| serde_json::from_value::<RequestPermissionResponse>(v).ok())
        .map_or(Verdict::Reject, |r| verdict(&r.outcome))
}
