//! Routines: a helper's standing instructions, run when a time comes, an almanac event happens or
//! another job is done. The book and its rules are `docket_core::routine`'s; this is what the
//! scheduler decides over it, as values.
//!
//! - `units`: the systemd user timers, as text.
//! - `watch`: when to ask the almanac, what, and which events were already acted on.
//! - `settle`: what the host is told when the book changes.
//! - `due`: the events for the jobs that ended.
//! - `desk`: the routine a form asks for.

pub(super) mod desk;
pub(super) mod due;
pub(super) mod settle;
pub(super) mod units;
pub(super) mod watch;
