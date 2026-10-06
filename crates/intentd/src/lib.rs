//! intentd's library: everything the daemon is made of except `main`, so each part is tested
//! without a bus. The router is pure (`docket-router`); this crate gives it its seams.
//!
//! - `IntentdConfig`: which bus names play which role, and which reviewers are installed.
//! - `builtin_manifests`, `MemoryProvider`, `CompanionProvider`, `HostedLink`: the providers
//!   intentd hosts, and the `AppLink` that answers them in process.
//! - `AlmanacMemory`: the router's memory seam over memoryd; [`QueuedSink`], [`record_of`] and
//!   `AuditLog`: the event sink, its mapping into almanac's bodies and its way to memoryd.
//! - `DbusLink`, `SheetConfirmer`, `FileGrants`, `InferdModel`, `ReaderClient`: the
//!   system implementations of the other seams.
//! - `SystemSeams`: all of them behind `docket_router::Seams`.
//! - `serve`, `serve_on`: the bus (`bus`: one handler per `Intents1` interface); `Peers`: who is on
//!   the other end of a connection; `end_terminals_at_logout`: logind; `start` and `run`: the daemon.

mod audit;
mod builtin;
mod builtin_companion;
mod builtin_memory;
mod bus;
mod config;
mod daemon;
mod defaults;
mod grants;
mod infer;
mod link;
mod logout;
mod manifests;
mod memory;
mod peer;
mod procroot;
mod reader_client;
mod record;
mod reviewers;
mod serve;
mod settings_watch;
mod sheet;
mod signals;
mod sink;
mod system;
mod writer;

pub use audit::{AuditLog, Flushed};
pub use builtin::{HostedLink, builtin_manifests, is_builtin};
pub use builtin_companion::{CompanionPort, CompanionProvider};
pub use builtin_memory::{MEMORY_APP, MemoryProvider};
pub use config::{ConfigError, IntentdConfig};
pub use daemon::{DaemonFault, Running, Setup, data_dirs, run, start};
pub use defaults::{DEFAULT_GRANTS_FILE, default_grants_file, revoking};
pub use grants::FileGrants;
pub use infer::{InferdModel, InferdWriter, ReaderClient, inferd_transport};
pub use link::DbusLink;
pub use logout::{Logouts, end_terminals_at_logout, watch_logind};
pub use manifests::{Loaded, intents_dir, load_manifests};
pub use memory::AlmanacMemory;
pub use peer::{PeerFault, Peers};
pub use procroot::{PROC_ROOT_VAR, ProcRoot, TestProcRoot, proc_root_choice};
pub use record::{kind_tag_of, record_of};
pub use reviewers::{placeholder_set, reviewer};
pub use serve::{ServeFault, serve, serve_on, serve_on_with};
pub use settings_watch::{DEBOUNCE, SettingsWatch, WatchState, apply, apply_next};
pub use sheet::SheetConfirmer;
pub use signals::{Cadence, Changed, Marks, changes, emit, marks_of, pump, rescan};
pub use sink::{QUEUE_LIMIT, QueuedSink};
pub use system::{SystemClock, SystemSeams};
