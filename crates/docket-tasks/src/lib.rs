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
mod plan;
mod records;
mod recover;
mod resume;
mod runtime;
mod seams;
mod shared;
mod sources;
mod task;

pub use act::{Acting, card_id};
pub use completion::completion_effects;
pub use drive::refusal_of;
pub use fault::ServeFault;
pub use recover::{RecentSource, ReplayFault, RouterRecent, recover, replay_of, restart_query};
pub use runtime::{Begun, Companion};
pub use seams::{Now, Quiet, Surface};
pub use shared::{Change, Shared};
pub use task::{Failure, TaskRuntime, kept_all};
