//! companiond: the companion as one identity over many tasks. It owns the front pointer (the
//! task the launcher returns to), the roster, the side-conversation tracker and the idle pass;
//! each task is its own docket session, with its own taint, budget and task policy, opened
//! through `Intents1` in the role `companion`. The pure machines are `agent-loop`'s; this crate
//! carries their effects out.
//!
//! - `Companiond`: the runtime and the entry points the bus calls.
//! - `PlannerModel`: the planner over inferd.
//! - `ReplaySource`, `recover`: restart, from what the eventlog holds.
//! - `completion_effects`: how a finished worker or run reaches the front task.
//! - `serve`: `org.quire.Companion1`.

mod completion;
mod planner;
mod recover;
mod runtime;
mod serve;

pub use completion::completion_effects;
pub use planner::{PlanFault, PlannerModel};
pub use recover::{ReplayFault, ReplaySource, recover};
pub use runtime::Companiond;
pub use serve::{ServeFault, serve};
