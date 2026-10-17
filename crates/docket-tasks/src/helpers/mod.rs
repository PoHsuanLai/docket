//! Helpers on the task model: what a helper's job needs of a `Companion` that the front
//! conversation does not. The board itself is `docket_core::helper`'s, owned by companiond; this
//! is the part that talks to the router and to a model.
//!
//! - `store`: the helpers' records, written through the router and read back after a restart.

mod store;

pub use store::{helper_query, helper_records_of};
