//! Routines: a helper's standing instructions, run when a time comes, an almanac event happens or
//! another job is done. The book and its rules are `docket_core::routine`'s (pure); this is the
//! small timer inside companiond that owns the book and carries out what it asks.
//!
//! - [`Routines`], [`Host`]: the scheduler and the seam to its surroundings.
//! - [`RoutineDesk`]: what the shell asks (list, card, make, pause, resume, remove).
//! - [`units_of`], [`UnitFile`], [`UnitDir`]: the systemd user timers, as text and on disk.
//! - [`HubHost`], [`Timers`]: the hub as the host in the daemon.

mod desk;
mod host;
mod hub_host;
mod scheduler;
mod units;
mod zone;

pub use desk::RoutineDesk;
pub use host::{Awake, EventAsk, Host, Seen};
pub use hub_host::{HubHost, Timers};
pub use scheduler::Routines;
pub use units::{UnitDir, UnitFile, units_of};
pub use zone::SystemZone;
