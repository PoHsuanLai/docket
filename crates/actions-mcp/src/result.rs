//! What a finished action tells a client: the app's words and the value it returned. The preview
//! (which may quote content), the undo token and the follow-up are the router's and the person's,
//! not the client's.

use docket_core::Outcome;
use serde_json::{Value as Json, json};

/// The JSON of an outcome: `{ "said": <text or null>, "value": <Value or null> }`.
pub fn outcome_json(outcome: &Outcome) -> Json {
    json!({
        "said": outcome.said.as_ref().map(|s| s.as_str()),
        "value": outcome.value.as_ref().map(|held| &held.value),
    })
}
