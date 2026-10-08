//! The ACP client edge: an external agent as a session backend whose calls go through the real
//! router, against a scripted fake agent over an in-memory pipe. Every test is scripted: no
//! process, no network, no clock; the real file system appears only under a scratch directory.

mod agent;
mod bridged;
mod calls;
mod contract;
mod full;
mod hostile;
mod hostile_limits;
mod osfiles;
mod rig;
mod schema;
mod world;
