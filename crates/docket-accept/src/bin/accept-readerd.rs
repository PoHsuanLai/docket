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
    match runtime.block_on(readerd::run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("readerd: {why}");
            ExitCode::FAILURE
        }
    }
}
