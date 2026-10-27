//! `docket-live`: the live-eval harness. `corpus` plays the eval corpora through the router with
//! the real policy writer and reviewer cascade over a model; `smoke` plays the acceptance flows
//! with every daemon against a model. The model is `--engine scripted|local|cloud`. It runs on a
//! private bus with scratch HOME and XDG directories; the network is touched only by
//! `--engine cloud`, and that is said before anything starts. Start it through
//! `scripts/eval-release.sh` or `dev/live-smoke.sh`.

use docket_accept::world::Binaries;
use docket_live::cli::{UsageError, parse};
use docket_live::command::run;
use std::process::ExitCode;

#[tokio::main(flavor = "multi_thread", worker_threads = 4)]
async fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let command = match parse(&words) {
        Ok(command) => command,
        Err(why @ UsageError::Subcommand) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
        Err(why) => {
            eprintln!("docket-live: {why}");
            return ExitCode::from(2);
        }
    };
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(why) => {
            eprintln!("docket-live: {why}");
            return ExitCode::from(2);
        }
    };
    let binaries = Binaries::beside(&exe, &|key: &str| std::env::var(key).ok());
    run(command, &binaries).await.unwrap_or_else(|why| {
        eprintln!("docket-live: {why}");
        ExitCode::from(2)
    })
}
