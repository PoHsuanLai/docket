//! `docket-agent --serve` as its own process under a name of the acceptance harness: the library's
//! `run_with`, given the fixture `/proc`, the system's callers file and the sandbox's limits from
//! the world's environment. The packaged `docket-agent` reads none of them.

use docket_accept::seams;
use docket_acp_bin::agent::serve::{self, HarnessSeams};
use docket_acp_bin::agent::wire::SandboxLimits;
use std::process::ExitCode;

fn main() -> ExitCode {
    let limits = if seams::switched_on(seams::AGENT_PER_PROCESS_LIMITS_VAR) {
        SandboxLimits::PerProcess
    } else {
        SandboxLimits::Scope
    };
    let chosen = HarnessSeams {
        proc_root: seams::proc_root(seams::AGENT_PROC_ROOT_VAR),
        system_callers: seams::system_callers(seams::AGENT_SYSTEM_CALLERS_VAR),
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
    match runtime.block_on(serve::run_with(chosen, |key: &str| std::env::var(key).ok())) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("docket-agent: {why}");
            ExitCode::from(2)
        }
    }
}
