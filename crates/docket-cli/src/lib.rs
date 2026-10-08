//! `quire-do`: every app we ship is drivable from a command line generated from its intents
//! manifest. This crate is a thin client over `docket-client` with caller role `cli`: it lists
//! the installed manifests, maps `--param value` words onto the manifest's declared types, and
//! asks intentd to perform, dry-run, search, read context or undo. It never talks to an app, never
//! decides, and has no way around the gate: a terminal cannot tell the person from an agent typing
//! in it, so every argument is untrusted and every act that is not a read asks the person.
//!
//! `run` is the whole program over any `Transport`; `main` supplies the bus.

mod args;
mod ask;
mod ask_render;
mod complete;
mod exec;
pub mod exit;
mod help;
mod outcome;
pub mod params;
pub mod program;
mod render;
pub mod resolve;
pub mod schema;
mod sessions;
mod skills;
pub mod when;

pub use args::{CallArgs, Command, JsonFlag, Parsed, RunMode, UndoWhich, parse};
pub use ask::ask as ask_companion;
pub use exec::Style;
pub use exit::{Exit, Failure};
pub use params::Stdin;
pub use resolve::Apps;
pub use sessions::SessionsCmd;
pub use skills::list as skills_list;

use docket_client::{Intents, Transport};
use exec::Printed;
use params::StdinSlot;

/// Whether standard output is a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stdout {
    /// A terminal: text, unless `--json`.
    Tty,
    /// A pipe or a file: JSON.
    Pipe,
}

/// One run of the program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    /// The command line, without the program name.
    pub words: Vec<String>,
    /// Standard input.
    pub stdin: Stdin,
    /// What standard output is.
    pub stdout: Stdout,
}

/// What the program prints and how it ends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Standard output.
    pub stdout: String,
    /// Standard error.
    pub stderr: String,
    /// The exit code.
    pub exit: Exit,
}

impl Report {
    fn said(text: String) -> Self {
        Self {
            stdout: format!("{text}\n"),
            stderr: String::new(),
            exit: Exit::Done,
        }
    }

    fn failed(failure: &Failure, style: Style) -> Self {
        match style {
            Style::Json => Self {
                stdout: format!("{}\n", failure.json()),
                stderr: String::new(),
                exit: failure.exit,
            },
            Style::Human => Self {
                stdout: String::new(),
                stderr: format!("quire-do: {}\n", failure.what),
                exit: failure.exit,
            },
        }
    }
}

/// Whether a command line needs intentd at all: help, the version and a command line that does
/// not parse are answered without a bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bus {
    /// It does.
    Needed,
    /// It does not.
    NotNeeded,
}

/// Whether `words` need the bus.
pub fn bus_for(words: &[String]) -> Bus {
    match parse(words) {
        Ok(Parsed {
            command: Command::Help | Command::Version,
            ..
        })
        | Err(_) => Bus::NotNeeded,
        Ok(_) => Bus::Needed,
    }
}

/// The transport of a run that must not reach intentd: every request finds it closed.
#[derive(Debug, Clone, Copy)]
pub struct NoBus;

impl Transport for NoBus {
    async fn call(
        &self,
        _: docket_core::IntentsRequest,
    ) -> Result<docket_core::IntentsReply, docket_client::TransportError> {
        Err(docket_client::TransportError::Closed)
    }
}

async fn apps_of<T: Transport>(intents: &Intents<T>) -> Result<Apps, Failure> {
    intents
        .manifests()
        .await
        .map(Apps::new)
        .map_err(|e| exit::of_client(&e))
}

async fn command<T: Transport>(
    intents: &Intents<T>,
    parsed: &Parsed,
    stdin: Stdin,
    roots: &docket_skills::Roots,
) -> Result<Printed, Failure> {
    let plain = |text: String| Printed {
        human: text.clone(),
        json: serde_json::Value::String(text),
    };
    match &parsed.command {
        Command::Help => Ok(plain(help::text())),
        Command::Version => Ok(plain(format!("quire-do {}", env!("CARGO_PKG_VERSION")))),
        Command::Undo(which) => exec::undo(intents, *which).await,
        Command::Ask { .. } => Err(Failure::new(
            Exit::Unavailable,
            "ask talks to the companion, not to intentd: it needs the companion's bus",
        )),
        Command::Sessions(SessionsCmd::List) => sessions::list(intents).await,
        Command::Sessions(SessionsCmd::Load(id)) => sessions::load(intents, id).await,
        Command::Sessions(SessionsCmd::Fork { session, at }) => {
            sessions::fork(intents, session, *at).await
        }
        Command::Apps => Ok(exec::apps(&apps_of(intents).await?)),
        Command::Skills => Ok(skills::list(&apps_of(intents).await?, roots)),
        Command::List { app } => exec::list(&apps_of(intents).await?, app),
        Command::Describe { app, action } => exec::describe(&apps_of(intents).await?, app, action),
        Command::Search { app, text } => {
            exec::search(intents, &apps_of(intents).await?, app, text).await
        }
        Command::Context { app } => exec::context(intents, &apps_of(intents).await?, app).await,
        Command::Call(args) => {
            let apps = apps_of(intents).await?;
            exec::call(intents, &apps, args, &mut StdinSlot::new(stdin)).await
        }
        Command::Complete(typed) => {
            let found = complete::candidates(&apps_of(intents).await?, typed);
            Ok(plain(found.join("\n")))
        }
    }
}

/// The whole program with no skill directories: `skills` lists none.
pub async fn run<T: Transport>(intents: &Intents<T>, invocation: Invocation) -> Report {
    run_in(intents, invocation, &docket_skills::Roots::default()).await
}

/// The whole program: parse the command line, ask intentd, print, and say how it ended. `roots`
/// are the skill directories `main` read from the environment, for `skills`.
pub async fn run_in<T: Transport>(
    intents: &Intents<T>,
    invocation: Invocation,
    roots: &docket_skills::Roots,
) -> Report {
    let parsed = match parse(&invocation.words) {
        Ok(parsed) => parsed,
        Err(failure) => {
            let style = match invocation.stdout {
                Stdout::Pipe => Style::Json,
                Stdout::Tty => Style::Human,
            };
            return Report::failed(&failure, style);
        }
    };
    let style = match (parsed.json, invocation.stdout) {
        (JsonFlag::Asked, _) | (_, Stdout::Pipe) => Style::Json,
        (JsonFlag::Auto, Stdout::Tty) => Style::Human,
    };
    let plain_text = matches!(
        parsed.command,
        Command::Help | Command::Version | Command::Complete(_)
    );
    match command(intents, &parsed, invocation.stdin, roots).await {
        Ok(printed) if plain_text => Report::said(match &printed.json {
            serde_json::Value::String(text) => text.clone(),
            other => other.to_string(),
        }),
        Ok(printed) => Report::said(printed.text(style)),
        Err(failure) => Report::failed(&failure, style),
    }
}
