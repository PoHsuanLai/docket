//! The editor's permission request. At a call's start it is an extra gate on top of ours: it
//! offers `allow_once` and one reject, never `allow_always` or `reject_always`; an allow lets the
//! call go on to our gate and is not a grant, a receipt, or an answer to a sheet.
//!
//! The router's own sheet, when the host hands it to the editor, is a second request. It adds
//! `allow_always` only when the sheet's offer is `Offered`, named for that typed scope, and a
//! selection of it when it was not offered is a reject. In both, an answer that names anything
//! else, an error, or a cancel is a reject.

use crate::calls;
use crate::scope_words::always_words;
use agent_client_protocol_schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionOutcome, RequestPermissionRequest,
    RequestPermissionResponse, SessionId, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use docket_core::{AlwaysOffer, ConfirmRequest};
use docket_session::{CallOpen, SheetChoice};
use serde_json::Value;

/// The id of the one option that lets a call go on.
pub const ALLOW_ONCE: &str = "allow_once";
/// The id of the option that lets this and later matching calls go on, offered on a sheet only.
pub const ALLOW_ALWAYS: &str = "allow_always";
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

/// The options of a sheet: `allow_always` is there only when the router offered it, and says
/// what it covers.
pub fn sheet_options(sheet: &ConfirmRequest) -> Vec<PermissionOption> {
    let always = match &sheet.always {
        AlwaysOffer::Offered(scope) => Some(PermissionOption::new(
            ALLOW_ALWAYS,
            always_words(sheet.action.as_str(), scope),
            PermissionOptionKind::AllowAlways,
        )),
        AlwaysOffer::Withheld(_) => None,
    };
    let mut options = vec![PermissionOption::new(
        ALLOW_ONCE,
        "Allow once",
        PermissionOptionKind::AllowOnce,
    )];
    options.extend(always);
    options.push(PermissionOption::new(
        REJECT_ONCE,
        "Reject",
        PermissionOptionKind::RejectOnce,
    ));
    options
}

/// The request for the router's `sheet`, named by the manifest's label for the action.
pub fn sheet_request(session: &SessionId, sheet: &ConfirmRequest) -> RequestPermissionRequest {
    let fields = ToolCallUpdateFields::new()
        .title(sheet.action.as_str().to_owned())
        .status(ToolCallStatus::Pending);
    let tool = ToolCallUpdate::new(format!("sheet-{}", sheet.id.as_str()), fields);
    RequestPermissionRequest::new(session.clone(), tool, sheet_options(sheet))
}

/// The choice in an outcome on `sheet`: anything but a listed allow is a refusal, and `always`
/// counts only if the sheet offered it.
pub fn choice(outcome: &RequestPermissionOutcome, sheet: &ConfirmRequest) -> SheetChoice {
    match outcome {
        RequestPermissionOutcome::Selected(chosen) => match &*chosen.option_id.0 {
            ALLOW_ONCE => SheetChoice::Once,
            ALLOW_ALWAYS if matches!(sheet.always, AlwaysOffer::Offered(_)) => SheetChoice::Always,
            _ => SheetChoice::Refused,
        },
        _ => SheetChoice::Refused,
    }
}

/// The choice in a reply to our request: anything unreadable is a refusal.
pub fn choice_of_reply(reply: Result<Value, Value>, sheet: &ConfirmRequest) -> SheetChoice {
    reply
        .ok()
        .and_then(|v| serde_json::from_value::<RequestPermissionResponse>(v).ok())
        .map_or(SheetChoice::Refused, |r| choice(&r.outcome, sheet))
}
