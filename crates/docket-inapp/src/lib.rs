//! The portable in-app agent (quire design/36): everything docket does for one app, hosted in
//! that app's own process. The app hands over its `IntentProvider`, a model transport, a
//! confirm sheet it draws itself and a clock; [`InAppAgent`] runs the router (gate, Cedar
//! policy point, reviewer), the agent loop and the planner over them, and answers a turn.
//!
//! No intentd, no bus, no daemon, no Unix socket: the crate builds with `--no-default-features`
//! on macOS and Windows. On our desktop the same app also serves its provider to intentd, and
//! the companion there reaches it across apps; this is the part that works without any of it.
//!
//! - [`InAppAgent`]: hosts the turn; `ask` runs it end to end and returns a [`Reply`].
//! - [`ConfirmSheet`]: what the app implements to ask its person; [`SheetConfirmer`] mints the
//!   receipt itself, so an app's sheet can say yes or no but never forge a proof.
//! - [`ProviderLink`], [`InAppSeams`]: the router's seams over the one provider, with the
//!   in-memory consent store ([`SessionGrants`]) and audit buffer ([`AuditBuffer`]).
//! - [`NoMemory`], [`NoReader`], [`NoWriter`]: the seams an app has no portable answer for yet.

mod agent;
mod link;
mod seams;
mod sheet;
mod turn;

pub use agent::{AgentFault, Ending, InAppAgent, InAppParts, Reply};
pub use link::ProviderLink;
pub use seams::{AuditBuffer, InAppSeams, NoMemory, NoReader, NoWriter, SessionGrants};
pub use sheet::{ConfirmSheet, SheetAnswer, SheetConfirmer};
