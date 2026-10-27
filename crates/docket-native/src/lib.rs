//! The native session backend and host: the planner loop of a `docket-tasks` `Companion` behind
//! `docket_session`'s `SessionBackend` and `SessionHost`, the durable-session backend of the ACP
//! edge. See `backend` for the two rules a backend keeps (a cancel-safe `next_event`, and no call
//! before its announcement is read).

mod backend;
mod end;
mod ending;
mod flight;
mod host;

pub use backend::{Core, NativeBackend};
pub use host::NativeHost;
