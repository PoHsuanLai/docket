//! readerd: the quarantined reader, serving `org.quire.Reader1` on the session bus until the bus
//! closes.

use std::process::ExitCode;

fn main() -> ExitCode {
    let _log = docket_log::init(&docket_log::Config::from_env("readerd"));
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
            tracing::error!("stopped: {why}");
            eprintln!("readerd: {why}");
            ExitCode::FAILURE
        }
    }
}
