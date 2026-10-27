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
//! - `Routines`, `RoutineDesk`: a helper's standing instructions, run by a small timer here.
//! - `HostLink`: a turn handed to the coding agent the person chose, through the agent host.
//! - `AppsLink`: the agent apps that are helpers, through the same host (`AgentApps` over the bus).

mod apps_link;
mod bell;
mod callers;
mod config;
mod daemon;
mod follow;
mod handoff;
mod helpers;
mod helpers_bus;
mod routines;
mod run_signals;
mod serve;
mod speaker;

pub use apps_link::AppsLink;
pub use bell::Bell;
pub use callers::{FILE as CALLERS_FILE, shipped as shipped_callers};
pub use config::{CompaniondConfig, ConfigError};
pub use daemon::{Daemon, HarnessSeams, run, run_with, start, start_with, start_with_agents};
pub use docket_planner::{
    ArgsFault, Catalogue, CatalogueTool, PlanFault, PlannerModel, PlannerReply, RULES, ReadCall,
    TARGET, TOOL_ANSWER, TOOL_ASK, TOOL_FINISH, TOOL_READ, TOOL_RELATED, messages, planner_label,
    read_call, system_text, user_text,
};
pub use docket_tasks::{
    Begun, Change, RecentSource, ReplayFault, RouterLog, RouterRecent, ServeFault, TaskRuntime,
    completion_effects, events_of, rebuild_from, recent_events, recover, replay_of, restart_query,
    stored_events,
};
pub use handoff::HostLink;
pub use helpers::{Hub, Told};
pub use routines::{
    Awake, EventAsk, Host, HubHost, RoutineDesk, Routines, Seen, SystemZone, Timers, UnitDir,
    UnitFile, units_of,
};
pub use serve::{serve, serve_on, serve_on_rooted, serve_with_routines};
pub use speaker::{Call, Speaker, Speakers, permits};

/// The shared view as the bus reads it.
pub type Shared = docket_tasks::Shared<Bell>;

/// The companion as the daemon and its tests run it: the task model over the system clock (or a
/// test's hand) and the bell.
pub type Companiond<P, I, K = docket_store::SystemClock> = docket_tasks::Companion<P, I, K, Bell>;
