//! docket's disk and timers, the client/IO layer under the pure `docket-core`:
//!
//! - [`write_atomic`] and [`read_optional`]: the one durable way to replace a file, and to read
//!   one that may not exist yet;
//! - the grant file's disk rules ([`read_grant_list`], [`write_grant_list`], [`GrantFileError`])
//!   and the [`WriteLock`] a grant store holds as a field;
//! - the waits on a peer with a deadline and a cancel ([`within`], [`until`], [`Cancel`],
//!   [`Waits`]) and the timer a host with no runtime sleeps on ([`thread_sleep`]);
//! - [`SystemClock`]: the one system implementation of `docket_vocab::Clock`.
//!
//! It names no app, window or label, and nothing here reads the environment.

mod atomic_file;
mod clock;
mod grant_file;
mod grant_lock;
mod wait;
mod wait_timer;

pub use atomic_file::{read_optional, write_atomic};
pub use clock::SystemClock;
pub use grant_file::{GrantFileError, read_grant_file, read_grant_list, write_grant_list};
pub use grant_lock::WriteLock;
pub use wait::{Cancel, Stop, ThreadSleep, Waits, stop_after, thread_sleep, until, within};
