//! The portable in-app agent (quire design/36): everything docket does for one app, hosted in
//! that app's own process. The app hands over its `IntentProvider`, a model transport, a
//! confirm sheet it draws itself and a clock; [`InAppAgent`] runs the router (gate, Cedar
//! policy point, reviewer), the agent loop and the planner over them, and answers a turn.
//!
//! No intentd, no bus, no daemon, no Unix socket: the crate builds with `--no-default-features`
//! on macOS and Windows. On our desktop the same app also serves its provider to intentd, and
//! the companion there reaches it across apps; this is the part that works without any of it.
//!
//! - [`InAppAgent`]: hosts the tasks (`docket-tasks`, the model companiond runs): `ask` runs a turn
//!   end to end and returns a [`Reply`]; `new_task`, `ask_in`, `front`, `roster`, `told`, `tick` and
//!   `restore` are the many-task doors; `install_skills` and `install_skills_from` add skills (words
//!   for the planner, never a grant); [`AuditFile`] keeps the audit records waiting for memory
//!   across a restart.
//! - [`ConfirmSheet`]: what the app implements to ask its person; [`SheetConfirmer`] mints the
//!   receipt itself, so an app's sheet can say yes or no but never forge a proof.
//! - [`ProviderLink`], [`InAppSeams`]: the router's seams over the one provider, with the
//!   in-memory consent store ([`SessionGrants`]) and audit buffer ([`AuditBuffer`]).
//! - [`NoMemory`], [`NoReader`], [`NoWriter`]: the stubs an app starts with.
//! - [`InAppKit`]: the real parts an app may choose instead, each portable and each over a
//!   transport the app hands in: [`FileGrantStore`] (consent that survives a restart),
//!   [`AlmanacMemory`] (recall and the audit as episodes, over `almanac-client`),
//!   [`TransportWriter`] (the task policy from the person's words), [`TransportReader`] (the
//!   quarantined reader in this process), and [`reviewer_over`] (the model-backed reviewer for
//!   [`InAppParts::reviewer`]); [`SystemClock`] is the router's clock with no runtime needed.

mod agent;
mod ask;
mod audit;
mod audit_file;
mod clock;
mod desk;
mod grants;
mod kit;
mod link;
mod seams;
mod sheet;
mod skills;
mod tasks;

pub use agent::{AgentFault, Ending, Failure, HostClock, InAppAgent, InAppParts, Reply};
pub use audit_file::{AuditFile, AuditFileError};
pub use clock::SystemClock;
pub use desk::EditorDesk;
pub use docket_memory::{AlmanacMemory, AuditState, Flushed, QueuedSink, Report};
pub use docket_models::{TransportModel, TransportWriter, placeholder_set, reviewer_over};
pub use docket_reader::TransportReader;
pub use grants::{FileGrantStore, GrantFileError};
pub use kit::{AuditTo, InAppKit};
pub use link::ProviderLink;
pub use seams::{AuditBuffer, InAppSeams, NoMemory, NoReader, NoWriter, SessionGrants};
pub use sheet::{ConfirmSheet, SheetAnswer, SheetConfirmer};
