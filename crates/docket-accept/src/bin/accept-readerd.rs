//! readerd as its own process: the packaged main, under a name of the acceptance harness.

use std::process::ExitCode;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(why) => {
            eprintln!("readerd: no runtime: {why}");
            return ExitCode::FAILURE;
        }
    };
    let env = |key: &str| std::env::var(key).ok();
    let seams = readerd::HarnessSeams {
        proc_root: docket_accept::seams::proc_root(
            &env,
            docket_accept::seams::READERD_PROC_ROOT_VAR,
        ),
        system_callers: docket_accept::seams::system_callers(
            &env,
            docket_accept::seams::READERD_SYSTEM_CALLERS_VAR,
        ),
    };
    match runtime.block_on(readerd::run_with(
        docket_accept::seams::trace(&env),
        seams,
        &env,
    )) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("readerd: {why}");
            ExitCode::FAILURE
        }
    }
}
