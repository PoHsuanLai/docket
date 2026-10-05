//! Test only: the agent tier's first end-to-end acceptance. The real `intentd`, `companiond`,
//! `readerd` and `memoryd` run as processes on a private bus, with a scripted model, a mail
//! provider and a `Confirm1` server around them (see `dev/accept/README.md`).

pub mod confirm;
pub mod drive;
pub mod grants;
pub mod inferd;
pub mod provider;
pub mod world;
