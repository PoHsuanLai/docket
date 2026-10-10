//! companiond: the companion as one identity over many tasks, on the session bus. The task model
//! is `docket-tasks`' (the front pointer, the roster, the side-conversation tracker, the idle
//! pass, restart, each task its own docket session); this crate is its adapter for the desktop:
//! the system clock, a [`Bell`] surface that fans changes out to the bus, `Companion1` served,
//! the speaker (who may call what) and the daemon.
//!
//! - `Companiond`: the runtime as the bus runs it (`docket_tasks::Companion` over the system
//!   clock and the bell).
//! - `PlannerModel`: the planner over inferd; `Catalogue`: the actions it may call.
//! - `RecentSource`, `recover`, `replay_of`: restart, from what the eventlog holds (messages,
//!   episodes, runs); `stored_events`, `events_of`: the sessions, from `Session.Stored`.
//! - `completion_effects`: how a finished worker or run reaches the front task.
//! - `serve`, `serve_on`: `org.quire.Companion1`.

mod bell;
mod clock;
mod config;
mod daemon;
mod follow;
mod serve;
mod speaker;

pub use bell::Bell;
pub use clock::Clock;
pub use config::{CompaniondConfig, ConfigError};
pub use daemon::{Daemon, run, start, start_with};
pub use docket_planner::{
    ArgsFault, Catalogue, CatalogueTool, PlanFault, PlannerModel, PlannerReply, RULES, ReadCall,
    TARGET, TOOL_ASK, TOOL_FINISH, TOOL_READ, TOOL_RELATED, messages, planner_label, read_call,
    system_text, user_text,
};
pub use docket_tasks::{
    Begun, Change, RecentSource, ReplayFault, RouterLog, RouterRecent, ServeFault, TaskRuntime,
    completion_effects, events_of, rebuild_from, recent_events, recover, replay_of, restart_query,
    stored_events,
};
pub use serve::{serve, serve_on, serve_on_rooted};
pub use speaker::{Call, PROC_GATE, PROC_ROOT_VAR, Speaker, permits};

/// The shared view as the bus reads it.
pub type Shared = docket_tasks::Shared<Bell>;

/// The companion as the daemon and its tests run it: the task model over the system clock (or a
/// test's hand) and the bell.
pub type Companiond<P, I> = docket_tasks::Companion<P, I, Clock, Bell>;
