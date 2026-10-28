//! The router, pure over its seams. It owns the rules that make the companion safe to run:
//! who may call what (`auth`), the order in which a call is refused or let through (`gate`),
//! tasks and the messages between them (`messages`) and the order of every request through the
//! seams the daemon passes in. What it remembers and shows (the sessions, the handle table, the
//! undo journal, the registry) is `docket-context`. `Router` ties them to the seams (`seams`).
//!
//! No clock, bus, file or runtime is reached here; the daemon (`intentd`) passes them in.
//!
//! A router over the neutral fake apps, asked what is installed (any caller may ask):
//!
//! ```
//! # #[tokio::main(flavor = "current_thread")]
//! # async fn main() -> Result<(), Box<dyn std::error::Error>> {
//! use docket_core::AgentConfig;
//! use docket_vocab::CallerRole;
//! use docket_wire::{CallerId, IntentsReply, IntentsRequest};
//! use docket_fake::fake_router;
//! use porter_core::{AppId, AppName, Isolation};
//! use std::collections::BTreeSet;
//!
//! let router = fake_router(AgentConfig::default())?;
//! let caller = CallerId {
//!     app: AppId {
//!         name: AppName::parse("org.quire.Shell")?,
//!         isolation: Isolation::Unsandboxed,
//!     },
//!     roles: BTreeSet::from([CallerRole::Editor]),
//! };
//! let reply = router.handle(&caller, IntentsRequest::Manifests).await;
//! assert!(matches!(reply, IntentsReply::Manifests(ref all) if !all.is_empty()));
//! # Ok(()) }
//! ```

mod argcheck;
mod assess;
mod auth;
mod brief;
mod brief_sheet;
mod call;
mod checkpoint_read;
mod checkpoint_restore;
mod checkpoint_sheet;
mod checkpoint_turn;
mod classify;
mod companion;
mod companion_names;
mod confirm;
mod consent;
mod control;
mod conversation;
mod coverage;
mod deadline;
mod driven;
mod dryrun;
mod durable;
mod facts;
mod filled;
mod finish;
mod gate;
mod gatecheck;
mod handover;
mod history;
mod limits;
mod may_make;
mod messages;
mod messaging;
mod mode;
mod notes;
mod opening;
mod perform;
mod perform_call;
mod perform_sheet;
mod policy;
mod policylog;
mod prepare;
mod prepared;
mod rank;
mod reading;
mod recall;
mod related;
mod restore;
mod router;
mod seams;
mod search;
mod shield;
mod shipping;
mod skills;
mod spans;
mod standing;
mod standing_ctx;
mod standing_facts;
mod stored;
mod terminal;
mod turn_running;
mod turn_start;
mod usage;
mod watch;
mod watched;

pub use auth::{acting_role, permits};
pub use call::{ArgsRefused, CallEffect, CallEvent, CallState, call_step};
pub use checkpoint_restore::{CHECKPOINTS_APP, CHECKPOINTS_RESTORE};
pub use checkpoint_sheet::restore_sheet;
pub use classify::{at_ceiling, classify_step, delegation_end};
// The store of a build that cannot keep history, for the seams of a portable host.
pub use companion_names::{BRIEF_CONFIRM, HELPER_POST_JOB, HELPER_PROPOSE, ROUTINE_PROPOSE};
pub use docket_checkpoint::NoStore;
pub use facts::{RunFacts, budget_fact, repeat_fact, run_facts, taint_fact};
pub use gate::{GateInputs, Pending, gate};
pub use messages::{assemble, check_stamped, inbound_line, intake_label, report_status};
pub use restore::{RestoreFault, Restored};
pub use router::Router;
pub use seams::{AppFault, AppLink, GrantStore, NoLog, Seams};
pub use skills::SKILL_LOAD;
pub use terminal::Revoked;
pub use usage::TurnBudget;
pub use watch::{Flag, Queue, Watch};
