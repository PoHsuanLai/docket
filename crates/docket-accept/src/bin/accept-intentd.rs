//! intentd as its own process: the packaged main, under a name of the acceptance harness.

use std::process::ExitCode;

fn main() -> ExitCode {
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(why) => {
            eprintln!("intentd: no runtime: {why}");
            return ExitCode::FAILURE;
        }
    };
    let env = |key: &str| std::env::var(key).ok();
    let seams = intentd::HarnessSeams {
        proc_root: docket_accept::seams::proc_root(
            &env,
            docket_accept::seams::INTENTD_PROC_ROOT_VAR,
        )
        .map_or(intentd::ProcRoot::System, intentd::ProcRoot::Fake),
        system_callers: docket_accept::seams::system_callers(
            &env,
            docket_accept::seams::INTENTD_SYSTEM_CALLERS_VAR,
        ),
        trace: docket_accept::seams::trace(&env),
        floor: None,
        admin_permissions: None,
    };
    match runtime.block_on(intentd::run_with(seams, &env)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("intentd: {why}");
            ExitCode::FAILURE
        }
    }
}
