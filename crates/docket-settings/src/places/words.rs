//! Person-facing text of the "where it may run" rows: plain words, no protocol names, no model
//! ids. The Settings app draws these strings as they are.

use super::model::{PlaceKind, PlaceName};

/// The heading of a kind of place.
pub fn kind_label(kind: PlaceKind) -> &'static str {
    match kind {
        PlaceKind::ThisComputer => "On this computer",
        PlaceKind::OwnComputer => "Your computers",
        PlaceKind::Cloud => "Cloud accounts",
    }
}

/// The confirmation before a cloud account is turned on.
pub fn confirm_turn_on(account: &PlaceName, provider: &str) -> String {
    format!(
        "Turn on {}? Requests the assistant can't do on your computers may be sent to {provider}.",
        account.0
    )
}

/// What the assistant says when no allowed place can do a task, offering to allow one.
pub fn no_place_says(would_need: PlaceKind) -> &'static str {
    match would_need {
        PlaceKind::ThisComputer => "This computer can't do that. Want to look at the options?",
        PlaceKind::OwnComputer => {
            "None of your allowed computers can do that. Allow another of your computers?"
        }
        PlaceKind::Cloud => "Nothing on your computers can do that. Allow a cloud account to help?",
    }
}
