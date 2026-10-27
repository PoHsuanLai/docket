//! Installing coding agents from the agent registry. Docket ships no list of agents: it reads a
//! registry snapshot (`agents-core`), installs the version the person pinned (checked against the
//! digest the registry gives) into a directory it owns, and says what is installed.
//!
//! - `install`: `install` (a named version), and `standing` and `offers` (what the list offers
//!   against what is installed). Updating is installing the newer version the person names; nothing runs by
//!   itself, and the network is reached only through `fetch::Fetch` in these two.
//! - `fetch`, `run`, `unpack`, `uv`: the download, the package-manager process, the archive and the
//!   python environment, each behind a seam the tests fake.
//! - `store`: the install record and the offered record on disk, and what is installed.

pub mod fetch;
pub mod install;
pub mod run;
pub mod store;
pub mod unpack;
pub mod uv;

pub use install::{InstallFault, Offer, Standing, Want, install, offers, standing};
