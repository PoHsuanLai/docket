//! The wording of the editor's permission request. At a call's start it is an extra gate on top of
//! ours: it offers `allow_once` and one reject, never `allow_always` or `reject_always`; an allow
//! lets the call go on to our gate and is not a grant, a receipt, or an answer to a sheet.
//!
//! The router's own sheet, when the host hands it to the editor, is a second request. It adds
//! `allow_always` only when the sheet's offer is `Offered`, named for that typed scope. How an
//! answer is ruled on (only `allow_once` allows; anything else is a reject) is `acp_core::permission`.

use crate::calls;
use crate::scope_words::always_words;
use acp_core::permission::{ALLOW_ALWAYS, ALLOW_ONCE, REJECT_ONCE};
use agent_client_protocol::schema::v1::{
    PermissionOption, PermissionOptionKind, RequestPermissionRequest, SessionId, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields,
};
use docket_core::{AlwaysOffer, ConfirmRequest};
use docket_session::CallOpen;

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
