//! Coding agents from the agent registry. Docket ships no list of agents: it reads a registry
//! snapshot, installs the version the person pinned (checked against the digest the registry
//! gives) into a directory it owns, and says how to start it.
//!
//! - `snapshot`: the registry's JSON, typed.
//! - `install`: `install` (a named version) and `standing` (what the list offers against what is
//!   installed). Updating is installing the newer version the person names; nothing runs by
//!   itself, and the network is reached only through `fetch::Fetch` in these two.
//! - `dirs`, `record`, `launch`: where agents live, what was installed, how to start it. Each
//!   agent has its own home inside its sandbox, where it signs in.
//! - `offered`: what an agent last offered when a session opened (its models and ways of signing
//!   in, and whether it was signed in), kept beside its home for the Settings app to read.

pub mod dirs;
pub mod fetch;
pub mod hash;
pub mod install;
pub mod launch;
pub mod offered;
pub mod platform;
pub mod record;
pub mod run;
pub mod slug;
pub mod snapshot;
pub mod unpack;
pub mod uv;

pub use dirs::AgentsDir;
pub use install::{InstallFault, Standing, Want, install, standing};
pub use launch::{Launch, LaunchFault};
pub use offered::{Offered, OfferedFault, Start};
pub use snapshot::{Listing, Snapshot, SnapshotFault};
