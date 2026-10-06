//! The whole `quire-do` program: the session bus as the transport, and the terminal as stdin and
//! stdout. `main` is `program::main`, and so is the acceptance harness's copy of the binary.

use crate::{
    Bus, Command, Exit, Invocation, NoBus, Report, Stdin, Stdout, ask_companion, bus_for, parse,
    run_in,
};
use companion_client::DbusCompanion;
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

fn unavailable(what: &impl std::fmt::Display) -> Report {
    Report {
        stdout: String::new(),
        stderr: format!("quire-do: {what}\n"),
        exit: Exit::Unavailable,
    }
}

/// `quire-do ask`: intentd records the turn, companiond is asked; both on the session bus.
async fn ask(text: &str, space: prov::SpaceId, json: crate::JsonFlag, stdout: Stdout) -> Report {
    let intents = match DbusTransport::connect().await {
        Ok(transport) => Intents::over(transport),
        Err(why) => return unavailable(&why),
    };
    let companion = match DbusCompanion::connect().await {
        Ok(companion) => companion,
        Err(why) => return unavailable(&why),
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
    ask_companion(
        &intents,
        &companion,
        text,
        space,
        prov::UnixSeconds(now),
        stdout,
        json,
    )
    .await
}

async fn report(words: Vec<String>) -> Report {
    let stdout = if std::io::stdout().is_terminal() {
        Stdout::Tty
    } else {
        Stdout::Pipe
    };
    if let Ok(parsed) = parse(&words)
        && let Command::Ask { text, space } = &parsed.command
    {
        return ask(text, space.clone(), parsed.json, stdout).await;
    }
    let stdin = stdin_for(&words);
    let roots = docket_skills::Roots::from_env(&|key| std::env::var(key).ok());
    if bus_for(&words) == Bus::NotNeeded {
        let invocation = Invocation {
            words,
            stdin,
            stdout,
        };
        return run_in(&Intents::over(NoBus), invocation, &roots).await;
    }
    match DbusTransport::connect().await {
        Ok(transport) => {
            run_in(
                &Intents::over(transport),
                Invocation {
                    words,
                    stdin,
                    stdout,
                },
                &roots,
            )
            .await
        }
        Err(why) => {
            let failure = crate::exit::of_client(&why.into());
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

/// Runs the program on this process's arguments, standard streams and session bus.
pub fn main() -> ExitCode {
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
