//! A private session bus for one test: one shared helper, whose daemon is killed by PID when the
//! bus drops (see `docket-testbus`). The person's real bus is never named.

#![allow(unused_imports)]

pub use docket_testbus::PrivateBus;
