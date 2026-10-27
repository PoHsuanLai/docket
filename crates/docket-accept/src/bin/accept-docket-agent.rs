//! `docket-agent --serve` as its own process under a name of the acceptance harness: the library's
//! `run_with`, given the fixture `/proc`, the system's callers file and the sandbox's limits from
//! the world's environment. The packaged `docket-agent` reads none of them.

use docket_accept::seams;
use docket_acp_bin::harness::host::SandboxLimits;
use docket_acp_bin::harness::serve::{HarnessSeams, run_with};
use std::process::ExitCode;

fn main() -> ExitCode {
    let env = |key: &str| std::env::var(key).ok();
    let limits = if seams::switched_on(&env, seams::AGENT_PER_PROCESS_LIMITS_VAR) {
        SandboxLimits::PerProcess
    } else {
        SandboxLimits::Scope
    };
    let chosen = HarnessSeams {
        proc_root: seams::proc_root(&env, seams::AGENT_PROC_ROOT_VAR),
        system_callers: seams::system_callers(&env, seams::AGENT_SYSTEM_CALLERS_VAR),
        limits,
    };
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(why) => {
            eprintln!("docket-agent: no runtime: {why}");
            return ExitCode::FAILURE;
        }
    };
    match runtime.block_on(run_with(chosen, env)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("docket-agent: {why}");
            ExitCode::from(2)
        }
    }
}
