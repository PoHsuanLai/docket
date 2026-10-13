//! `docket-bringup up|down|status`: the agent services on a demo shell's private bus. The logic
//! is `docket_accept::bringup`; see `docs/demo-bringup.md`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    docket_accept::bringup::run_cli(&args)
}
