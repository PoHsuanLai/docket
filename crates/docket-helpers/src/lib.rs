//! The helpers' runner. companion-core decides over values (how a job's end is told to the board,
//! the kept answers, the desk's refusals, the routines' due and event rules); `docket-tasks` is
//! the task model a job runs on. This crate owns the state and carries the effects out, with no
//! bus of its own:
//!
//! - [`Hub`]: the one owner of the helpers' board. It applies the board's steps, writes the
//!   records that changed through the router, tells the person the sentences worth telling, keeps
//!   each helper's conversation, and runs each job as a task of its own through [`Spawn`]. It is
//!   the `HelperDesk` the planner's helper actions and the shell reach.
//! - [`Spawn`], [`Fate`], [`Cutoff`]: how the hub starts tasks and cuts one off. `TokioSpawn`
//!   (feature `tokio`) is the one for a tokio runtime.
//! - [`Routines`], [`Host`], [`RoutineDesk`]: a helper's standing instructions, run by a small
//!   timer; [`HubHost`] is the hub as the routines' host, [`UnitDir`] the systemd user timers on
//!   disk, [`SystemZone`] the local zone, [`Timers`] whether the timers are written.
//!
//! The plain sentence of a refusal is made in `words`, one place for both desks.

mod hub;
mod routines;
mod spawn;
mod words;

pub use hub::Hub;
pub use routines::{Host, HubHost, RoutineDesk, Routines, SystemZone, Timers, UnitDir};
#[cfg(feature = "tokio")]
pub use spawn::TokioSpawn;
pub use spawn::{Cutoff, Fate, Spawn, Task, Watch, Work};
