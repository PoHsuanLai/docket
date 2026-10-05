//! companiond: the companion as one identity over many tasks. It owns the front pointer (the
//! task the launcher returns to), the roster, the side-conversation tracker and the idle pass;
//! each task is its own docket session, with its own taint, budget and task policy, opened
//! through `Intents1` in the role `companion`. The pure machines are `agent-loop`'s; this crate
//! carries their effects out.
//!
//! - `Companiond`: the runtime and the entry points the bus calls.
//! - `PlannerModel`: the planner over inferd; `Catalogue`: the actions it may call.
//! - `RecentSource`, `recover`, `replay_of`: restart, from what the eventlog holds, read through
//!   `Recent` with `BodyMode::Json`.
//! - `completion_effects`: how a finished worker or run reaches the front task.
//! - `serve`, `serve_on`: `org.quire.Companion1`.

mod act;
mod args;
mod catalogue;
mod clock;
mod completion;
mod config;
mod daemon;
mod drive;
mod fault;
mod finish;
mod idle;
mod inbox;
mod plan;
mod planner;
mod records;
mod recover;
mod render;
mod resume;
mod runtime;
mod serve;
mod shared;
mod sources;
mod task;

pub use args::{ArgsFault, ReadCall, planner_label, read_call};
pub use catalogue::{Catalogue, CatalogueTool, TARGET};
pub use clock::Clock;
pub use completion::completion_effects;
pub use config::{CompaniondConfig, ConfigError};
pub use daemon::{Daemon, run, start};
pub use fault::ServeFault;
pub use planner::{PlanFault, PlannerModel, PlannerReply, TOOL_ASK, TOOL_FINISH, TOOL_READ};
pub use recover::{RecentSource, ReplayFault, RouterRecent, recover, replay_of, restart_query};
pub use render::{RULES, messages, system_text, user_text};
pub use runtime::{Begun, Companiond};
pub use serve::{serve, serve_on};
pub use shared::{Change, Shared};
pub use task::TaskRuntime;
