//! `docket-net-enter [--read P]... [--write P]... [--device P]... NETWORK -- <program> [args...]`
//!
//! The exec stage every agent starts behind, which bulkhead provides as a function: it limits
//! what the agent may do with its files and its network, says how far that went on its standard
//! error, and starts the agent. The host starts it from beside itself (`agent::sibling`).

fn main() -> std::process::ExitCode {
    bulkhead::enter::enter_main("docket-net-enter")
}
