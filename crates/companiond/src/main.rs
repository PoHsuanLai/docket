//! companiond: serves `org.quire.Companion1` on the session bus until the bus closes.

use std::process::ExitCode;

fn main() -> ExitCode {
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
    match runtime.block_on(companiond::run()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("companiond: {why}");
            ExitCode::FAILURE
        }
    }
}
