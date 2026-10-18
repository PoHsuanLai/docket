//! Helpers: the board of jobs and the mailbox, owned here. companiond is the single owner, so
//! taking a job is decided in the board's state machine (`docket_core::helper::step`) and needs no
//! compare-and-set; this carries its effects out: the records written through the router (and
//! read back at a restart), the notices the person is shown, and the tasks that run the jobs.
//!
//! - [`Hub`], [`Told`]: the board, its records, the sentences the person is told and the tasks that
//!   run the jobs, each on a companion of its own.

mod desk;
mod hub;
mod proposals;

pub use hub::{Hub, Told};
