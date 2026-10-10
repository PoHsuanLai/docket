//! Agent checkpoints, as data: the restore points of a session, what the log says about them,
//! what restoring would change, and why a turn has none. Pure and portable (serde only); the
//! store that keeps the points is `docket-checkpoint`'s seam.
//!
//! None of the person-facing wording is here: callers draw it from the typed enums, so no free
//! text crosses the wire.

mod event;
mod id;
mod list;
mod path;
mod plan;
mod turn;

pub use event::{CheckpointEvent, CheckpointNote, Rewind, SkipReason};
pub use id::CheckpointId;
pub use list::{CheckpointList, CheckpointRow, Retention, SavedState};
pub use path::{WorkPath, WorkPathError};
pub use plan::{CheckpointFault, PlanDigest, RestorePlan};
pub use turn::{TurnEnd, TurnState};
