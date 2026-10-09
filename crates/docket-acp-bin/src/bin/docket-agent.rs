//! `docket-agent PROGRAM [--cwd DIR] [--tty]`: see `docket_acp_bin::agent`.

use docket_acp_bin::agent::{args, run};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let here = std::env::current_dir().unwrap_or_default();
    let Some(args) = args::parse(std::env::args().skip(1), here) else {
        eprintln!("usage: docket-agent PROGRAM [--cwd DIR] [--tty | --refresh]");
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
