//! Minting and reading a [`HelperId`] through its serde form alone (a string), so the id's own
//! constructor and accessors are named nowhere else in docket.

use prov::HelperId;
use serde_json::Value;

/// The prefix of every helper id docket mints.
const PREFIX: &str = "h-";

/// The id `h-<n>`, if it is a valid [`HelperId`].
pub(crate) fn helper_id(n: u64) -> Option<HelperId> {
    serde_json::from_value(Value::String(format!("{PREFIX}{n}"))).ok()
}

/// The number in an id docket minted (`h-<n>`); none for any other id.
pub(crate) fn helper_number(id: &HelperId) -> Option<u64> {
    match serde_json::to_value(id).ok()? {
        Value::String(text) => text.strip_prefix(PREFIX)?.parse().ok(),
        _ => None,
    }
}
