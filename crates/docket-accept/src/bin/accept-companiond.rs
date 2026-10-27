//! companiond as its own process: the packaged main, under a name of the acceptance harness.

use std::process::ExitCode;

fn main() -> ExitCode {
    let _log = docket_log::init(&docket_log::Config::from_env("companiond"));
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(why) => {
            eprintln!("companiond: no runtime: {why}");
            return ExitCode::FAILURE;
        }
    };
    let defaults = companiond::HarnessSeams::default();
    let seams = companiond::HarnessSeams {
        proc_root: docket_accept::seams::proc_root(docket_accept::seams::COMPANIOND_PROC_ROOT_VAR)
            .unwrap_or(defaults.proc_root),
        system_callers: docket_accept::seams::system_callers(
            docket_accept::seams::COMPANIOND_SYSTEM_CALLERS_VAR,
        ),
        trace: docket_accept::seams::trace(),
    };
    match runtime.block_on(companiond::run_with(seams)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("companiond: {why}");
            ExitCode::FAILURE
        }
    }
}
