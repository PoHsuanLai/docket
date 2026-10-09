//! Test only: the agent tier's first end-to-end acceptance. The real `intentd`, `companiond`,
//! `readerd`, `memoryd` and `inferd` run as processes on a private bus (inferd playing a cassette),
//! with a mail provider and a `Confirm1` server around them (see `dev/accept/README.md`).

pub mod acp;
pub mod confirm;
pub mod drive;
pub mod grants;
pub mod live;
pub mod provider;
mod runlink;
pub mod things;
pub mod world;
