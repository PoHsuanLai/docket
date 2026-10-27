//! `docket-agent PROGRAM [--cwd DIR] [--tty]`, or `docket-agent --serve`: see
//! `docket_acp_bin::agent`.

use docket_acp_bin::agent::args::Request;
use docket_acp_bin::agent::{args, run, serve};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let here = std::env::current_dir().unwrap_or_default();
    let args = match args::parse(std::env::args().skip(1), here) {
        Ok(Request::Serve) => {
            return match serve::run().await {
                Ok(()) => ExitCode::SUCCESS,
                Err(why) => {
                    eprintln!("docket-agent: {why}");
                    ExitCode::from(2)
                }
            };
        }
        Ok(Request::Run(args)) => args,
        Err(usage) => usage.exit(),
    };
    match run(args).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("docket-agent: {why}");
            ExitCode::from(2)
        }
    }
}
