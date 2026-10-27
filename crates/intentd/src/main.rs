//! intentd: serves `org.quire.Intents1` on the session bus until the bus closes.

use std::process::ExitCode;

fn main() -> ExitCode {
    docket_log::init("intentd");
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
    match runtime.block_on(intentd::run(&env)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("intentd: {why}");
            ExitCode::FAILURE
        }
    }
}
