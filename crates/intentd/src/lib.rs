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
//! - `serve`: the bus.

mod builtin;
mod config;
mod grants;
mod infer;
mod link;
mod memory;
mod record;
mod serve;
mod sheet;
mod sink;
mod system;

pub use builtin::{CompanionProvider, MemoryProvider, builtin_manifests};
pub use config::{ConfigError, IntentdConfig};
pub use grants::FileGrants;
pub use infer::{InferdModel, InferdWriter, ReaderClient, inferd_transport};
pub use link::DbusLink;
pub use memory::AlmanacMemory;
pub use record::{kind_tag_of, record_of};
pub use serve::{ServeFault, serve};
pub use sheet::SheetConfirmer;
pub use sink::QueuedSink;
pub use system::{SystemClock, SystemSeams};
