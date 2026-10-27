//! The `quire-do` binary: collects what the process holds and hands it to `docket_cli::program`.

use docket_cli::program::{self, Process};
use docket_cli::{Exit, SessionBus, Stdin, Stdout};
use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;
use std::time::SystemTime;

fn read_stdin() -> Stdin {
    let mut text = String::new();
    match std::io::stdin().read_to_string(&mut text) {
        Ok(_) => Stdin::Text(text),
        Err(_) => Stdin::Closed,
    }
}

fn env(key: &str) -> Option<String> {
    std::env::var(key).ok()
}

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    let process = Process {
        words: std::env::args().skip(1).collect(),
        stdout: match std::io::stdout().is_terminal() {
            true => Stdout::Tty,
            false => Stdout::Pipe,
        },
        stdin: read_stdin,
        env,
        now: SystemTime::now(),
    };
    let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        return ExitCode::from(Exit::Unavailable.code());
    };
    let done = runtime.block_on(async {
        tokio::spawn(program::execute(process, SessionBus))
            .await
            .unwrap_or_else(|_| program::broken("quire-do stopped on an internal error"))
    });
    let _ = std::io::stdout().write_all(done.stdout.as_bytes());
    let _ = std::io::stderr().write_all(done.stderr.as_bytes());
    ExitCode::from(done.exit.code())
}
