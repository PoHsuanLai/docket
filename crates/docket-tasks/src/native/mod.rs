//! The native session backend and host: the planner loop of a `Companion` behind
//! `docket_session`'s `SessionBackend` and `SessionHost`. See `backend` for the two rules a
//! backend keeps (a cancel-safe `next_event`, and no call before its announcement is read).

mod backend;
mod end;
mod flight;
mod host;
mod log;
mod stored;

pub use backend::{Core, NativeBackend};
pub use host::NativeHost;
pub use log::RouterLog;
