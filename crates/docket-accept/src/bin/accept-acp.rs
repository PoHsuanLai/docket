//! docket-acp as its own process, switched on: the library's `serve` under a name of the
//! acceptance harness (the packaged binary reads the person's settings file; this is the
//! setting `agent.acp.expose = "on"` already read).

use docket_acp::Permit;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Ok(permit) = Permit::from_text("[agent.acp]\nexpose = \"on\"\n") else {
        return ExitCode::from(2);
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
