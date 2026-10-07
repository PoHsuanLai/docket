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
mod clock;
mod completion;
mod config;
mod daemon;
mod drive;
mod fault;
mod finish;
mod held;
mod idle;
mod inbox;
mod linger;
mod plan;
mod records;
mod recover;
mod resume;
mod runtime;
mod serve;
mod shared;
mod sources;
mod speaker;
mod step_text;
mod task;

pub use clock::Clock;
pub use completion::completion_effects;
pub use config::{CompaniondConfig, ConfigError};
pub use daemon::{Daemon, run, start, start_with};
pub use docket_planner::{
    ArgsFault, Catalogue, CatalogueTool, PlanFault, PlannerModel, PlannerReply, RULES, ReadCall,
    TARGET, TOOL_ASK, TOOL_FINISH, TOOL_READ, messages, planner_label, read_call, system_text,
    user_text,
};
pub use fault::ServeFault;
pub use recover::{RecentSource, ReplayFault, RouterRecent, recover, replay_of, restart_query};
pub use runtime::{Begun, Companiond};
pub use serve::{serve, serve_on, serve_on_rooted};
pub use shared::{Change, Shared};
pub use speaker::{Call, PROC_ROOT_VAR, Speaker, permits, proc_root_from};
pub use task::TaskRuntime;
