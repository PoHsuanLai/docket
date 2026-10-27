//! docket's memory seam over a transport: what the router asks of memory and what it leaves there,
//! over any `almanac_client::Transport` (memoryd over D-Bus on the desktop, the service hosted in
//! the app's own process elsewhere). The pure half, the audit records as almanac's events and the
//! bounded queue, is `docket-memory-core`.
//!
//! - [`AlmanacMemory`]: the router's `MemoryLink` over an almanac `Memory`.
//! - [`AlmanacSessionLog`]: a session's durable log (`docket-session`'s `SessionLog`) over almanac's
//!   `RecordDurable` and `Entries`.
//! - [`MemoryProvider`]: `org.quire.Memory`, the actions the planner reaches memory through, as an
//!   `IntentProvider` over an almanac `Memory`.
//! - [`AuditState`]: draining the queue into memory, Space by Space, with retry accounting.

mod audit;
mod cursor_cache;
mod memory;
mod provider;
mod session_log;

pub use audit::{AuditState, Flushed, Link, Report};
pub use memory::AlmanacMemory;
pub use provider::{MEMORY_APP, MemoryProvider};
pub use session_log::AlmanacSessionLog;
