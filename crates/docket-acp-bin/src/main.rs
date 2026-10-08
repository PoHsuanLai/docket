//! docket-acp: the ACP server edge as a process. An editor launches it and speaks ACP on its
//! stdin and stdout; logs go to stderr only. It refuses to serve unless the setting
//! `agent.acp.expose` is on. Exits 2 when that is off or the command line is bad, 1 for any other
//! stop.

use docket_acp::Permit;
use std::process::ExitCode;

const USAGE: &str =
    "usage: docket-acp   (no arguments: the router knows this process by its bus name)";

fn main() -> ExitCode {
    if std::env::args().nth(1).is_some() {
        eprintln!("docket-acp: {USAGE}");
        return ExitCode::from(2);
    }
    let permit = match Permit::from_env(&|k| std::env::var(k).ok()) {
        Ok(permit) => permit,
        Err(why) => {
            eprintln!("docket-acp: {why}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(why) => {
            eprintln!("docket-acp: no runtime: {why}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(docket_acp_bin::serve(permit)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("docket-acp: {why}");
            ExitCode::FAILURE
        }
    }
}
