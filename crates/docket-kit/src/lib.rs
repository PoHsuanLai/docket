//! Declaring an internal agent in a few lines (a reviewer, a policy writer, a worker, a test
//! agent) without a second loop. The kit is a facade over the machines docket already has:
//! `agent-loop`'s `agent_step` and `assemble`, `docket-planner`'s prompt and catalogue,
//! `docket-tasks`' memory reads and reader step, and the `Intents1` link. It adds typed choices
//! and the small driver that joins them for one task.
//!
//! What the kit does not have is as much of its design as what it has: an agent holds no tool
//! and no closure, so its only effect is a `CallRequest` sent through the router, which gates it
//! (labels, Cedar, review, confirmation, breaker, audit); nothing rewrites a call; a value a
//! call returns is a handle, never text in the next prompt; the rules in the system message
//! come first and are not an option of the builder.
//!
//! - [`Agent`], [`AgentBuilder`], [`Asker`]: declare and ask.
//! - [`Actions`]: the chosen actions, by app, name and greatest effect.
//! - [`Memory`], [`RecallScope`]: the sections it remembers, each opt-in.
//! - [`Limits`]: how far an ask may go.
//! - [`Run`], [`Ended`]: what an ask came to.

mod actions;
mod agent;
mod builder;
mod driver;
mod fault;
mod limits;
mod memory;
mod run;

pub use actions::{Actions, Missing};
pub use agent::{Agent, Asker};
pub use builder::AgentBuilder;
pub use fault::{BuildFault, KitFault};
pub use limits::Limits;
pub use memory::{EPISODE_LIMIT, Memory, RECALL_HITS, RecallScope};
pub use run::{Ended, Run};
