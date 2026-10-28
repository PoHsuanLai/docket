//! `docket-bringup`: the agent services (accountd, inferd, memoryd, intentd, companiond) on a bus that
//! already exists, for a demo shell whose companion pane should really answer. It reuses the
//! acceptance world's pieces: the same daemons, `env_clear` plus the scratch environment, the
//! same fake proc root and configuration files. Unlike a world, the daemons outlive this
//! process; `<root>/procs` records their pids and start times for `down` and `status`.
//!
//! No local model engine is ever configured: the `inferd.toml` it writes names no engine and
//! probes no local runtime port (see [`seed::NO_LOCAL_ENGINE`]). Nothing here opens a key file.
//! See `docs/demo-bringup.md`.

pub mod cli;
pub mod down;
pub mod procs;
pub mod refuse;
pub mod seed;
pub mod up;

use cli::{Command, UsageError};
use docket_accept::keys_home::HomeFault;
use docket_accept::world::{Binaries, env_of};
use procs::ProcsFault;
use refuse::{Own, Refusal};
use seed::ExtraFault;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Why a command failed.
#[derive(Debug, thiserror::Error)]
pub enum BringupError {
    /// A safety refusal.
    #[error("{0}")]
    Refused(#[from] Refusal),
    /// The key home is not usable.
    #[error("{0}")]
    Keys(#[from] HomeFault),
    /// The extra `inferd.toml` rows are not usable.
    #[error("--inferd-config: {0}")]
    Inferd(#[from] ExtraFault),
    /// The procs file is damaged.
    #[error("{0}")]
    Procs(#[from] ProcsFault),
    /// A file or directory could not be made, read or written.
    #[error("{0}")]
    Io(#[from] std::io::Error),
    /// A named input could not be used.
    #[error("{0}")]
    Config(String),
    /// The bus could not be reached.
    #[error("the bus: {0}")]
    Bus(String),
    /// Services of this root are still running.
    #[error("{0} (pid {1}) of this root is still running: run `docket-bringup down --root` first")]
    AlreadyUp(String, u32),
    /// A daemon binary is missing.
    #[error("{0} does not exist: build the daemons first (cargo build -p docket-accept --bins)")]
    NoBinary(PathBuf),
    /// A daemon's start time could not be read right after it started.
    #[error("{0} (pid {1}) vanished right after it started; see logs/{0}.log")]
    NoStartTime(&'static str, u32),
    /// A daemon exited before it was on the bus.
    #[error("{0} exited before it was on the bus ({1}); see logs/{0}.log in the root")]
    Exited(&'static str, String),
    /// A daemon never put its name on the bus.
    #[error("{0} never took {1} on the bus; see logs/{0}.log in the root")]
    NoName(&'static str, String),
}

fn quote(word: &str) -> String {
    let plain = |c: char| c.is_ascii_alphanumeric() || "/._-:=@,+".contains(c);
    match word.chars().all(plain) {
        true => word.to_owned(),
        false => format!("'{}'", word.replace('\'', "'\\''")),
    }
}

/// The command line that adds an OpenRouter key to a root whose accountd keeps its keys in the
/// root's own file (the run without `--keys-home`). accountd must be stopped while it runs.
pub fn add_command(root: &Path, accountd: &Path) -> String {
    let keys = format!("file:{}", root.join("keys/accountd.keys").display());
    let env: Vec<String> = env_of(root)
        .into_iter()
        .map(|(name, value)| format!("{name}={value}"))
        .chain([format!("ACCOUNTD_KEYS={keys}")])
        .map(|pair| quote(&pair))
        .collect();
    format!(
        "env -i {} {} add openrouter --allow org.quire.Companion --allow org.quire.Intents",
        env.join(" "),
        quote(&accountd.display().to_string())
    )
}

fn say_up(args: &cli::UpArgs, binaries: &Binaries, started: &[up::Started]) {
    started
        .iter()
        .for_each(|s| println!("{} {} ok", s.name, s.pid));
    if args.keys_home.is_none() {
        println!(
            "Cloud models need a key in this root; skip this if you already added one. To add it, stop the services (docket-bringup down --root {}), then type the key at accountd's own prompt:\n  {}\nthen run up again.",
            args.root.display(),
            add_command(&args.root, &binaries.accountd)
        );
        if args.providers.is_none() {
            println!(
                "The root has no provider files, so accountd add will not know OpenRouter: run up with --providers (an install keeps them in ~/.local/share/porter/providers) first."
            );
        }
    }
}

fn run_command(
    command: Command,
    exe: &Path,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<(), BringupError> {
    match command {
        Command::Up(args) => {
            let binaries = Binaries::beside(exe, env);
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            let started = runtime.block_on(up::up(&args, &binaries, &Own::read(env)))?;
            say_up(&args, &binaries, &started);
        }
        Command::Down(args) => {
            for done in down::down(&args.root)? {
                println!(
                    "{} {} {}",
                    done.record.name,
                    done.record.pid,
                    done.outcome.word()
                );
            }
        }
        Command::Status(args) => {
            for (record, state) in down::status(&args.root)? {
                println!("{} {} {}", record.name, record.pid, state.word());
            }
        }
    }
    Ok(())
}

/// The whole program: parses `args` (without the program's name), runs the command and prints
/// its lines. `exe` is the program's own path (the daemons are beside it) and `env` the environment
/// `main` hands in. Exit code 2 for a wrong command line, 1 for a failure.
pub fn run_cli(args: &[String], exe: &Path, env: &impl Fn(&str) -> Option<String>) -> ExitCode {
    let command = match cli::parse(args) {
        Ok(command) => command,
        Err(why @ UsageError::Subcommand) => {
            eprintln!("{why}");
            return ExitCode::from(2);
        }
        Err(why) => {
            eprintln!("docket-bringup: {why}");
            return ExitCode::from(2);
        }
    };
    match run_command(command, exe, env) {
        Ok(()) => ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("docket-bringup: {why}");
            ExitCode::FAILURE
        }
    }
}
