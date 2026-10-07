//! The models docket asks, portable: a porter-infer `Model` over any `porter_client::Transport`
//! (inferd over D-Bus on the desktop, the latchkey socket or `InProcess` elsewhere), the
//! task-policy writer and the reviewer cascade's construction. Moved out of intentd as
//! `docket-planner` was moved out of companiond, so an app-hosted agent builds the same model-backed
//! reviewer and writer with the transport it has; intentd's `InferdModel` and `InferdWriter` are
//! thin adapters that add the bus constructor.
//!
//! - [`TransportModel`]: chat and embeddings through a session per request.
//! - [`TransportWriter`]: the policy writer (`PolicyWriter`).
//! - [`reviewer_over`], [`placeholder_set`], [`card_of`]: the cascade of `action-review`.
//! - [`turn`], [`chat_of`], [`chat_need`]: the pieces the quarantined reader shares.

mod draft;
mod model;
mod reviewers;
mod writer;

pub use model::{Discard, TransportModel, chat_need, chat_of, embed_of, turn, unwanted};
pub use reviewers::{card_of, placeholder_set, reviewer_over};
pub use writer::TransportWriter;
