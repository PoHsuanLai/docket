//! docket's memory seam, portable: what the router asks of memory and what it leaves there, over
//! any `almanac_client::Transport` (memoryd over D-Bus on the desktop, the service hosted in the
//! app's own process elsewhere). Moved out of intentd as `docket-planner` was moved out of
//! companiond; intentd re-exports every name and adds the bus transport and the log lines.
//!
//! - [`AlmanacMemory`]: the router's `MemoryLink` over an almanac `Memory`.
//! - [`record_of`], [`kind_tag_of`]: the router's audit records as almanac's events.
//! - [`AlmanacSessionLog`]: a session's durable log (`docket-session`'s `SessionLog`) over almanac's
//!   `RecordDurable` and `Entries`.
//! - [`QueuedSink`]: the bounded event sink the host drains.
//! - [`AuditState`]: draining the sink into memory, Space by Space, with retry accounting.

mod audit;
mod cursor_cache;
mod memory;
mod record;
mod session_log;
mod sink;

pub use audit::{AuditState, Flushed, Link, Report};
pub use memory::AlmanacMemory;
pub use record::{kind_tag_of, record_of, space_named_by};
pub use session_log::AlmanacSessionLog;
pub use sink::{QUEUE_LIMIT, QueuedSink};
