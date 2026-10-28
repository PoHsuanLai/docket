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

mod assess;
mod auth;
mod brief;
mod checkpoint_read;
mod checkpoint_restore;
mod checkpoint_sheet;
mod checkpoint_turn;
mod companion;
mod control;
mod conversation;
mod deadline;
mod dryrun;
mod durable;
mod filled;
mod finish;
mod gatecheck;
mod handover;
mod history;
mod limits;
mod messages;
mod messaging;
mod mode;
mod notes;
mod opening;
mod perform;
mod perform_call;
mod perform_sheet;
mod policy;
mod prepare;
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
mod standing;
mod stored;
mod terminal;
mod turn_running;
mod usage;
mod watch;
mod watched;

pub use auth::{acting_role, permits};
pub use checkpoint_restore::{CHECKPOINTS_APP, CHECKPOINTS_RESTORE};
pub use checkpoint_sheet::restore_sheet;
// The store of a build that cannot keep history, for the seams of a portable host.
pub use docket_checkpoint::NoStore;
pub use messages::{assemble, check_stamped, inbound_line, intake_label, report_status};
pub use restore::{RestoreFault, Restored};
pub use router::Router;
pub use seams::{AppFault, AppLink, GrantStore, NoLog, Seams};
pub use skills::SKILL_LOAD;
pub use usage::TurnBudget;
pub use watch::{Flag, Queue, Watch};
