//! The `quire-do` binary: the session bus as the transport, and the terminal as stdin and stdout.

use docket_cli::{Bus, Exit, Invocation, NoBus, Report, Stdin, Stdout, bus_for, run};
use docket_client::{DbusTransport, Intents};
use std::io::{IsTerminal, Read, Write};
use std::process::ExitCode;

fn stdin_for(words: &[String]) -> Stdin {
    if !words.iter().any(|w| w == "-") {
        return Stdin::Closed;
    }
    let mut text = String::new();
    match std::io::stdin().read_to_string(&mut text) {
        Ok(_) => Stdin::Text(text),
        Err(_) => Stdin::Closed,
    }
}

async fn report(words: Vec<String>) -> Report {
    let stdout = if std::io::stdout().is_terminal() {
        Stdout::Tty
    } else {
        Stdout::Pipe
    };
    let stdin = stdin_for(&words);
    if bus_for(&words) == Bus::NotNeeded {
        let invocation = Invocation {
            words,
            stdin,
            stdout,
        };
        return run(&Intents::over(NoBus), invocation).await;
    }
    match DbusTransport::connect().await {
        Ok(transport) => {
            run(
                &Intents::over(transport),
                Invocation {
                    words,
                    stdin,
                    stdout,
                },
            )
            .await
        }
        Err(why) => {
            let failure = docket_cli::exit::of_client(&why.into());
            Report {
                stdout: String::new(),
                stderr: format!("quire-do: {}\n", failure.what),
                exit: failure.exit,
            }
        }
    }
}

/// What a run that panicked is reported as: a terminal should read "unavailable", not a
/// backtrace.
fn broken(why: &str) -> Report {
    Report {
        stdout: String::new(),
        stderr: format!("quire-do: unavailable: {why}\n"),
        exit: Exit::Unavailable,
    }
}

fn main() -> ExitCode {
    std::panic::set_hook(Box::new(|_| {}));
    let words: Vec<String> = std::env::args().skip(1).collect();
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => return ExitCode::from(Exit::Unavailable.code()),
    };
    let done = runtime.block_on(async {
        tokio::spawn(report(words))
            .await
            .unwrap_or_else(|_| broken("quire-do stopped on an internal error"))
    });
    let _ = std::io::stdout().write_all(done.stdout.as_bytes());
    let _ = std::io::stderr().write_all(done.stderr.as_bytes());
    ExitCode::from(done.exit.code())
}
