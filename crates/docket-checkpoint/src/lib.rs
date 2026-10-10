//! Restore points of an agent's workspace: pure rules plus one seam.
//!
//! - `CheckpointStore`: the seam. All file and history effects (save the folder as a point,
//!   list the points, plan and apply a restore, forget points) are its methods; the git store is
//!   `docket-checkpoint-git`, and `MemoryStore` and `NoStore` are here.
//! - `decide`, `next_id`, `retention`: the per-turn decision, the next point number and the
//!   points to forget, from the log's notes and the store's list alone.
//! - `plan_restore`: two tree listings in, a `RestorePlan` with its digest out.
//! - `contract` (feature `testing`): the rules every store keeps, written once for each store's
//!   test to run. Its checks panic by design, so a release build does not carry them.
//!
//! Nothing here reaches a file, a process, a bus or a clock: every time is passed in.

#[cfg(feature = "testing")]
pub mod contract;
mod ids;
mod memory;
mod rules;
mod store;
mod tree;

pub use ids::{EntryId, Saved, TreeId, WorkRoot};
pub use memory::{MemoryStore, Tracking};
pub use rules::{Decision, decide, next_id, retention};
pub use store::{
    ApplyAsk, CheckpointStore, DropAsk, NoStore, PlanAsk, StoreFault, TakeAsk,
};
pub use tree::{TreeListing, plan_restore};
