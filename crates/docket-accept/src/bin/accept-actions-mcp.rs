//! The bridge an external agent starts, as its own process: the packaged `actions-mcp` main, under
//! a name of the acceptance harness. The harness binds it into the agent's sandbox.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match actions_mcp::Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(why) => {
            eprintln!("actions-mcp: {why}");
            return ExitCode::from(2);
        }
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(why) => {
            eprintln!("actions-mcp: no runtime: {why}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(actions_mcp::run(args)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("actions-mcp: {why}");
            ExitCode::FAILURE
        }
    }
}
