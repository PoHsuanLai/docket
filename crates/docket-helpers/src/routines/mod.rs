//! Routines: a helper's standing instructions, run when a time comes, an almanac event happens or
//! another job is done. The book and its rules are `docket_core::routine`'s (pure), and what the
//! scheduler decides over it (the events for ended jobs, when and what to ask the almanac, the
//! timer text) is `companion_core`'s; this is the small timer inside the daemon that owns the
//! book and carries out what it asks.
//!
//! - [`Routines`], [`Host`]: the scheduler and the seam to its surroundings.
//! - [`RoutineDesk`]: what the shell asks (list, card, make, pause, resume, remove).
//! - [`UnitDir`]: the systemd user timers, on disk.
//! - [`HubHost`], [`Timers`]: the hub as the host in the daemon.

mod desk;
mod host;
mod hub_host;
mod scheduler;
mod unit_dir;
mod zone;

pub use desk::RoutineDesk;
pub use host::Host;
pub use hub_host::{HubHost, Timers};
pub use scheduler::Routines;
pub use unit_dir::UnitDir;
pub use zone::SystemZone;
