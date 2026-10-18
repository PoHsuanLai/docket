//! Helpers on the task model: what a helper's job needs of a `Companion` that the front
//! conversation does not. The board itself is `docket_core::helper`'s, owned by companiond; this
//! is the part that talks to the router and to a model.
//!
//! - `store`: the helpers' records, written through the router and read back after a restart.
//! - `desk_calls`: the planner's two helper actions, carried out through the host's desk.
//! - `job`: a helper's job as an ordinary task on a `Companion` of its own.
//! - `menu`: the actions and skills a helper's planner is offered, cut to its reach.
//! - `view`: the options its inference sessions open with and the role its planner works under.

mod desk_calls;
mod job;
mod menu;
mod store;
mod view;

pub(crate) use desk_calls::{Keeps, is_helper_action, offered_here};
pub use job::{JobEnding, JobInput, JobReport, JobSpec, OpenedJob};
pub use menu::AsHelper;
pub(crate) use menu::{SKILL_LOAD, in_menu, skill_in_menu};
pub use store::{helper_query, helper_records_of};
pub use view::{conversation_voice, open_options, planner_for, role_text};
