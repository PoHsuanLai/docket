//! The planner model, portable: what companiond asks inferd and what an app-hosted agent asks
//! its own model. The request is a function of the view and the catalogue alone; the transport
//! is any `porter_client::Transport` (inferd over D-Bus on the desktop, the latchkey socket or
//! `InProcess` elsewhere). Nothing here reaches a bus or a runtime.
//!
//! - `PlannerModel`: the planner; `converse` turns a view into one `PlannerReply`.
//! - `Catalogue`: the actions it may call, as tools.
//! - `messages`, `system_text`, `user_text`, `RULES`: the prompt; `RoleText`: an agent's own
//!   instructions, which follow the rules and never replace them.
//! - `read_call`, `planner_label`: the planner's own steps and labels.

mod args;
mod catalogue;
mod planner;
mod read_ask;
mod render;
mod role;
mod step_text;
mod unconfirmed;
mod want;

pub use args::{ArgsFault, ReadCall, planner_label, read_call};
pub use catalogue::{Catalogue, CatalogueTool, TARGET};
pub use planner::{PlanFault, PlannerModel, PlannerReply, TOOL_ASK, TOOL_FINISH, TOOL_READ};
pub use render::{RULES, messages, messages_with, system_text, system_text_with, user_text};
pub use role::{ROLE_LIMIT, RoleFault, RoleText};
