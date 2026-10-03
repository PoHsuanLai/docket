//! intentd's library: everything the daemon is made of except `main`, so each part is tested
//! without a bus. The router is pure (`docket-router`); this crate gives it its seams.
//!
//! - `IntentdConfig`: which bus names play which role, and which reviewers are installed.
//! - `builtin_manifests`, `MemoryProvider`, `CompanionProvider`: the providers intentd hosts.
//! - `AlmanacMemory`: the router's memory seam over memoryd; [`QueuedSink`] and [`record`]: the
//!   event sink and its mapping into almanac's bodies.
//! - `DbusLink`, `SheetConfirmer`, `FileGrants`, `InferdModel`, `ReaderClient`: the
//!   system implementations of the other seams.
//! - `SystemSeams`: all of them behind `docket_router::Seams`.
//! - `serve`, `serve_on`: the bus (`bus`: one handler per `Intents1` interface); `Peers`: who is on
//!   the other end of a connection; `end_terminals_at_logout`: logind; `start` and `run`: the daemon.

mod builtin;
mod bus;
mod config;
mod daemon;
mod grants;
mod infer;
mod link;
mod logout;
mod manifests;
mod memory;
mod peer;
mod record;
mod reviewers;
mod serve;
mod sheet;
mod sink;
mod system;

pub use builtin::{CompanionProvider, MemoryProvider, builtin_manifests};
pub use config::{ConfigError, IntentdConfig};
pub use daemon::{DaemonFault, Running, Setup, data_dirs, run, start};
pub use grants::FileGrants;
pub use infer::{InferdModel, InferdWriter, ReaderClient, inferd_transport};
pub use link::DbusLink;
pub use logout::{Logouts, end_terminals_at_logout, watch_logind};
pub use manifests::{Loaded, intents_dir, load_manifests};
pub use memory::AlmanacMemory;
pub use peer::{PeerFault, Peers};
pub use record::{kind_tag_of, record_of};
pub use reviewers::{placeholder_set, reviewer};
pub use serve::{ServeFault, serve, serve_on};
pub use sheet::SheetConfirmer;
pub use sink::QueuedSink;
pub use system::{SystemClock, SystemSeams};
