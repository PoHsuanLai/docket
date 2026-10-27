//! `docket-agent PROGRAM [--cwd DIR] [--tty]`, or `docket-agent --serve`: see
//! `docket_acp_bin::entry::docket_agent`.

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    docket_acp_bin::entry::docket_agent(
        std::env::args().skip(1),
        std::env::current_dir().unwrap_or_default(),
        |key: &str| std::env::var(key).ok(),
    )
    .await
}
