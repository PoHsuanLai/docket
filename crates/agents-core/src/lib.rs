//! The coding-agent registry as values: a registry snapshot, the names and digests it uses, the
//! layout of the directory docket keeps agents in, and the two small records kept there (what was
//! installed, what the agent last offered). Nothing here reads a disk, the network or the
//! environment; `docket-agents` installs and reads, and the Settings rows and the launcher read
//! the records through these types.
//!
//! - `slug`, `hash`, `platform`: a name safe as a directory, a SHA-256, a registry platform key.
//! - `snapshot`: the registry's JSON, typed.
//! - `dirs`: where agents live, as paths.
//! - `record`, `launch`: what was installed, and how to start it.
//! - `offered`: what an agent last offered when a session opened (its models and ways of signing
//!   in, and whether it was signed in).

pub mod dirs;
pub mod hash;
pub mod launch;
pub mod offered;
pub mod platform;
pub mod record;
pub mod slug;
pub mod snapshot;
