//! The gate's ruling on the editor's or an agent's permission request: which option ids mean
//! "go on", and what an outcome, a reply or an error is worth. Only `allow_once` allows; `allow_always`
//! counts only on a sheet that offered it; anything else, an error or a cancel is a reject. The
//! words of the options are not here.

use agent_client_protocol::schema::v1::{
    Error, RequestPermissionOutcome, RequestPermissionResponse,
};
use docket_core::ConfirmRequest;
use docket_permissions_core::AlwaysOffer;
use docket_session::SheetChoice;

/// The id of the one option that lets a call go on.
pub const ALLOW_ONCE: &str = "allow_once";
/// The id of the option that lets this and later matching calls go on, offered on a sheet only.
pub const ALLOW_ALWAYS: &str = "allow_always";
/// The id of the option that stops it.
pub const REJECT_ONCE: &str = "reject_once";

/// Whether the editor's person let the call go on, to our gate: only a selection of `allow_once`
/// does.
pub fn allows(outcome: &RequestPermissionOutcome) -> bool {
    matches!(
        outcome,
        RequestPermissionOutcome::Selected(chosen) if &*chosen.option_id.0 == ALLOW_ONCE
    )
}

/// Whether a reply to our request let the call go on: anything unreadable is a reject.
pub fn allows_reply(reply: Result<RequestPermissionResponse, Error>) -> bool {
    reply.is_ok_and(|r| allows(&r.outcome))
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
pub fn choice_of_reply(
    reply: Result<RequestPermissionResponse, Error>,
    sheet: &ConfirmRequest,
) -> SheetChoice {
    reply.map_or(SheetChoice::Refused, |r| choice(&r.outcome, sheet))
}
