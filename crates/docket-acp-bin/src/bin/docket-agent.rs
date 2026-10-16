//! `docket-agent PROGRAM [--cwd DIR] [--tty]`, or `docket-agent --serve`: see
//! `docket_acp_bin::agent`.

use docket_acp_bin::agent::{args, run, serve};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let mut words = std::env::args().skip(1);
    // The service the bus starts for the assistant: no program, no terminal.
    if words.next().as_deref() == Some("--serve") && words.next().is_none() {
        return match serve::run().await {
            Ok(()) => ExitCode::SUCCESS,
            Err(why) => {
                eprintln!("docket-agent: {why}");
                ExitCode::from(2)
            }
        };
    }
    let here = std::env::current_dir().unwrap_or_default();
    let Some(args) = args::parse(std::env::args().skip(1), here) else {
        eprintln!(
            "usage: docket-agent PROGRAM [--cwd DIR] [--tty | --refresh]   |   docket-agent --serve"
        );
        return ExitCode::from(2);
    };
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("docket-agent: {why}");
            ExitCode::from(2)
        }
    }
}
