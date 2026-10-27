//! Helpers: the board of jobs and the mailbox, owned here. The hub is the single owner, so
//! taking a job is decided in the board's state machine (`docket_core::helper::step`) and needs no
//! compare-and-set; this carries its effects out: the records written through the router (and
//! read back at a restart), the notices the person is shown, and the tasks that run the jobs.
//! What is decided over values (how a job's end is told to the board, the kept answers, the
//! desk's refusals) is `companion_core`'s.
//!
//! - `owner`: the board, its records, the sentences the person is told and each helper's
//!   conversation.
//! - `jobs`: the tasks that run the jobs, each on a companion of its own.
//! - `desk`, `hand`: the hub as the desk the shell and the planner talk to.
//! - `routine_link`: what the routines need of the hub.

mod desk;
mod hand;
mod jobs;
mod owner;
mod routine_link;

pub use owner::Hub;
