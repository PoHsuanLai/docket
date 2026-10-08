//! The companion's task model, portable: one identity over many tasks. It owns the front pointer
//! (the task the launcher returns to), the roster, the side-conversation tracker and the idle
//! pass; each task is its own docket session, with its own taint, budget and task policy, opened
//! through `Intents1` in the role `companion`. The pure machines are `agent-loop`'s; this crate
//! carries their effects out against a router link, a model transport, a clock and a surface.
//!
//! companiond is the bus adapter over it (the system clock, a surface that fans changes out on
//! the bus); `docket-inapp` hosts it in one app with the app's own clock and a quiet surface. No
//! bus, no runtime: a crate that needs to wait gets a future from its surface.
//!
//! - [`Companion`]: the runtime and the entry points a host calls.
//! - [`Now`], [`Surface`], [`Quiet`]: the seams beyond the link and the model.
//! - [`Shared`], [`Change`]: what a surface reads without waiting for the loop.
//! - [`RecentSource`], [`recover`], [`replay_of`]: restart, from what the eventlog holds, read
//!   through `Recent` with `BodyMode::Json`.
//! - [`NativeBackend`], [`NativeHost`]: the planner loop behind the durable-session traits
//!   (`docket-session`), the ACP edge's host.
//! - [`Tap`]: how a turn is told as it runs and a call is held at its gate.
//! - [`completion_effects`]: how a finished worker or run reaches the front task.

mod act;
mod completion;
mod drive;
mod fault;
mod finish;
mod held;
mod idle;
mod inbox;
mod linger;
mod native;
mod plan;
mod records;
mod recover;
mod resume;
mod runtime;
mod seams;
mod shared;
mod sources;
mod stored_roster;
mod tap;
mod task;

pub use act::{Acting, card_id};
pub use completion::completion_effects;
pub use drive::refusal_of;
pub use fault::ServeFault;
pub use native::{Core, NativeBackend, NativeHost, RouterLog};
pub use recover::{
    RecentSource, ReplayFault, RouterRecent, rebuild_from, recent_events, recover, replay_of,
    restart_query,
};
pub use runtime::{Begun, Companion};
pub use seams::{Now, Quiet, Surface};
pub use shared::{Change, Shared};
pub use stored_roster::{events_of, stored_events};
pub use tap::{Go, NoTap, Tap};
pub use task::{Failure, TaskRuntime, kept_all};
