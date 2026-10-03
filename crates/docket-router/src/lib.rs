//! The router, pure over its seams. It owns the rules that make the companion safe to run:
//! who may call what (`auth`), the order in which a call is refused or let through (`gate`),
//! the session machine with its breaker pause (`session`), the handle table that keeps
//! untrusted text away from planners (`handles`), the undo journal (`journal`), tasks and the
//! messages between them (`tasks`, `messages`), the shadow index machine (`index`) and the
//! manifest registry (`registry`). `Router` ties them to the seams (`seams`).
//!
//! No clock, bus, file or runtime is reached here; the daemon (`intentd`) passes them in.

mod argcheck;
mod auth;
mod call;
mod companion;
mod confirm;
mod consent;
mod control;
mod coverage;
mod deadline;
mod driven;
mod dryrun;
mod finish;
mod gate;
mod gatecheck;
mod handles;
mod index;
mod journal;
mod labels;
mod messages;
mod messaging;
mod notes;
mod opening;
mod perform;
mod policy;
mod prepare;
mod prepared;
mod reading;
mod recall;
mod registry;
mod router;
mod seams;
mod search;
mod session;
mod state;
mod tasks;
mod terminal;
mod watch;
mod who;

pub use auth::{acting_role, permits};
pub use call::{ArgsRefused, CallEffect, CallEvent, CallState, call_step};
pub use companion::COMPANION_APP;
pub use gate::{GateInputs, Pending, gate};
pub use handles::{HandleEntry, HandleTable, HandleValue, ViewInputs, context_view, planner_view};
pub use index::{IndexAction, IndexEvent, index_step};
pub use journal::UndoJournal;
pub use messages::{assemble, check_stamped, inbound_line, intake_label, report_status};
pub use registry::{Registry, RegistryError, parse};
pub use router::Router;
pub use seams::{AppFault, AppLink, Clock, EventSink, GrantStore, LinkFault, MemoryLink, Seams};
pub use session::{CloseCause, SessionEffect, SessionEvent, SessionState, Taint, session_step};
pub use state::{RouterState, SessionRecord};
pub use tasks::{TaskRecord, TaskState, TaskTable, child_policy, roster_of};
pub use terminal::Revoked;
pub use watch::{Flag, Queue, Watch};
