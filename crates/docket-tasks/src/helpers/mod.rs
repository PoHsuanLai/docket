//! Helpers on the task model: what a helper's job needs of a `Companion` that the front
//! conversation does not. The board itself is `docket_core::helper`'s, owned by companiond; this
//! is the part that talks to the router and to a model.
//!
//! - `store`: the helpers' records, written through the router and read back after a restart.
//! - `job`: a helper's job as an ordinary task on a `Companion` of its own.
//! - `view`: the options its inference sessions open with and the role its planner works under.

mod job;
mod store;
mod view;

pub use job::{JobEnding, JobInput, JobReport, JobSpec, OpenedJob};
pub use store::{helper_query, helper_records_of};
pub use view::{open_options, planner_for, role_text};
