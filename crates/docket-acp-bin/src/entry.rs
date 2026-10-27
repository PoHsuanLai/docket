//! The program entry points `main` wires: what a binary of this package calls once it has
//! collected its arguments and environment.

use crate::agent::args::{self, Request};
use crate::agent::{self, serve};
use std::path::PathBuf;
use std::process::ExitCode;

/// `docket-agent PROGRAM [--cwd DIR] [--tty]`, or `docket-agent --serve`. `argv` is the command
/// line after the program's name, `here` the working directory and `env` the environment `main`
/// hands in. A bad command line exits the process with clap's usage; a setting that is off or a
/// configuration that is wrong ends with code 2.
pub async fn docket_agent(
    argv: impl IntoIterator<Item = String>,
    here: PathBuf,
    env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
) -> ExitCode {
    let ended = match args::parse(argv, here) {
        Ok(Request::Serve) => serve::run(env).await,
        Ok(Request::Run(args)) => agent::run(args, env).await,
        Err(usage) => usage.exit(),
    };
    match ended {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("docket-agent: {why}");
            ExitCode::from(2)
        }
    }
}
