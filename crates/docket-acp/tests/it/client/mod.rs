//! The ACP client edge: an external agent as a session backend, against a scripted fake agent
//! over an in-memory pipe. Every test is scripted: no process, no network, no clock; the real
//! file system appears only under a scratch directory.

mod agent;
mod contract;
mod full;
mod hostile;
mod osfiles;
mod rig;
mod schema;
