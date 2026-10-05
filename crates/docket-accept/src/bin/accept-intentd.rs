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
    match runtime.block_on(intentd::run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("intentd: {why}");
            ExitCode::FAILURE
        }
    }
}
