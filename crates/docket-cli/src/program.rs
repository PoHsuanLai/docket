//! The whole `quire-do` program over what the process hands it: the command line, standard input
//! and output, the environment, the clock and the buses. Nothing here reads a global; `main` (and
//! the acceptance harness's copy of it) collects those and passes them in.

use crate::{
    Bus, Command, Exit, Invocation, JsonFlag, NoBus, Report, Stdin, Stdout, ask_companion, bus_for,
    exit, parse, run_in,
};
use companion_client::CompanionTransport;
use docket_client::{Intents, Transport, TransportError};
use prov::{SpaceId, UnixSeconds};
use std::future::Future;
use std::time::{SystemTime, UNIX_EPOCH};

/// Reads standard input to its end. Called only when the command line names `-`.
pub type ReadStdin = fn() -> Stdin;

/// An environment variable, if it is set.
pub type EnvLookup = fn(&str) -> Option<String>;

/// What the process hands the program.
#[derive(Debug, Clone)]
pub struct Process {
    /// The command line, without the program name.
    pub words: Vec<String>,
    /// What standard output is.
    pub stdout: Stdout,
    /// How to read standard input.
    pub stdin: ReadStdin,
    /// How to read the environment.
    pub env: EnvLookup,
    /// The time now.
    pub now: SystemTime,
}

/// Where the program reaches intentd and the assistant. A connection is made only when a command
/// needs it.
pub trait Buses: Send + Sync {
    /// The carrier of Intents1.
    type Intents: Transport;
    /// The carrier of the assistant's conversation calls.
    type Companion: CompanionTransport;

    /// Connects to intentd.
    fn intents(
        &self,
    ) -> impl Future<Output = Result<Intents<Self::Intents>, TransportError>> + Send;

    /// Connects to the assistant.
    fn companion(&self) -> impl Future<Output = Result<Self::Companion, TransportError>> + Send;
}

fn unavailable(what: &impl std::fmt::Display) -> Report {
    Report {
        stdout: String::new(),
        stderr: format!("quire-do: {what}\n"),
        exit: Exit::Unavailable,
    }
}

fn seconds(now: SystemTime) -> UnixSeconds {
    UnixSeconds(
        now.duration_since(UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0)),
    )
}

/// `quire-do ask`: intentd records the turn, companiond is asked.
async fn ask<B: Buses>(
    buses: &B,
    text: &str,
    space: SpaceId,
    json: JsonFlag,
    process: &Process,
) -> Report {
    let intents = match buses.intents().await {
        Ok(intents) => intents,
        Err(why) => return unavailable(&why),
    };
    let companion = match buses.companion().await {
        Ok(companion) => companion,
        Err(why) => return unavailable(&why),
    };
    ask_companion(
        &intents,
        &companion,
        text,
        space,
        seconds(process.now),
        process.stdout,
        json,
    )
    .await
}

/// Runs the program on what `process` hands it, and says what it prints and how it ends.
pub async fn execute<B: Buses>(process: Process, buses: B) -> Report {
    if let Ok(parsed) = parse(&process.words)
        && let Command::Ask { text, space } = &parsed.command
    {
        return ask(&buses, text, space.clone(), parsed.json, &process).await;
    }
    let stdin = match process.words.iter().any(|w| w == "-") {
        true => (process.stdin)(),
        false => Stdin::Closed,
    };
    let roots = docket_skills::Roots::from_env(&process.env);
    let bus = bus_for(&process.words);
    let invocation = Invocation {
        words: process.words,
        stdin,
        stdout: process.stdout,
    };
    match bus {
        Bus::NotNeeded => run_in(&Intents::over(NoBus), invocation, &roots).await,
        Bus::Needed => match buses.intents().await {
            Ok(intents) => run_in(&intents, invocation, &roots).await,
            Err(why) => {
                let failure = exit::of_client(&why.into());
                Report {
                    stdout: String::new(),
                    stderr: format!("quire-do: {}\n", failure.what),
                    exit: failure.exit,
                }
            }
        },
    }
}

/// What a run that panicked is reported as: a terminal should read "unavailable", not a
/// backtrace.
pub fn broken(why: &str) -> Report {
    Report {
        stdout: String::new(),
        stderr: format!("quire-do: unavailable: {why}\n"),
        exit: Exit::Unavailable,
    }
}
