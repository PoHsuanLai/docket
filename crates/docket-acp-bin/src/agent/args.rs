//! The command line of `docket-agent`: `docket-agent PROGRAM [--cwd DIR] [--space NAME] [--tty]`,
//! or `docket-agent PROGRAM --refresh`: start the agent briefly, write down what it offers, and
//! stop it without a turn. `docket-agent --serve` is the service the bus starts for the
//! assistant: no program, no terminal.

use clap::error::ErrorKind;
use clap::{ArgAction, CommandFactory, Parser};
use docket_acp_host::Fallback;
use docket_session::ProgramName;
use prov::SpaceId;
use std::path::PathBuf;

/// What the run is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// A session with a prompt on the terminal.
    Session,
    /// Open a session, write down what the agent offers, close it. No turn is taken.
    Refresh,
}

/// What was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The configured program (`agents.toml`).
    pub program: ProgramName,
    /// The directory it works in.
    pub cwd: PathBuf,
    /// The Space the session is in (the desktop's, unless `--space` names one).
    pub space: SpaceId,
    /// Where the person answers its sheets: the desktop's, unless `--tty` asked for this
    /// terminal (a development fallback).
    pub fallback: Fallback,
    /// A session, or a refresh of what the agent offers.
    pub mode: Mode,
}

/// What the command line asks the process to be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Request {
    /// `--serve`: the service the bus starts.
    Serve,
    /// A session with a program, or a refresh of what it offers.
    Run(Args),
}

/// The command line as clap reads it.
#[derive(Debug, Parser)]
#[command(
    name = "docket-agent",
    about = "Hosts a coding agent over ACP",
    no_binary_name = true
)]
struct Cli {
    /// The configured program (`agents.toml`).
    #[arg(value_parser = ProgramName::parse)]
    program: Option<ProgramName>,
    /// The service the bus starts for the assistant; takes nothing else.
    #[arg(long, action = ArgAction::Count)]
    serve: u8,
    /// The directory the program works in (the current one unless given).
    #[arg(long)]
    cwd: Option<PathBuf>,
    /// The Space the session is in (the desktop's unless given).
    #[arg(long, value_parser = SpaceId::parse)]
    space: Option<SpaceId>,
    /// Answer the agent's sheets on this terminal (a development fallback).
    #[arg(long, action = ArgAction::Count)]
    tty: u8,
    /// Open a session, write down what the agent offers, close it.
    #[arg(long, action = ArgAction::Count)]
    refresh: u8,
}

impl Cli {
    fn request(self, here: PathBuf) -> Result<Request, clap::Error> {
        let options =
            self.cwd.is_some() || self.space.is_some() || self.tty > 0 || self.refresh > 0;
        match (self.program, self.serve) {
            (None, 0) => Err(Self::command().error(
                ErrorKind::MissingRequiredArgument,
                "a PROGRAM is required, or --serve",
            )),
            (None, _) if options => {
                Err(Self::command()
                    .error(ErrorKind::ArgumentConflict, "--serve takes no other option"))
            }
            (None, _) => Ok(Request::Serve),
            (Some(_), 1..) => {
                Err(Self::command().error(ErrorKind::ArgumentConflict, "--serve takes no program"))
            }
            (Some(program), 0) => Ok(Request::Run(Args {
                program,
                cwd: self.cwd.unwrap_or(here),
                space: self.space.unwrap_or_else(SpaceId::desktop),
                fallback: match self.tty {
                    0 => Fallback::Off,
                    _ => Fallback::Terminal,
                },
                mode: match self.refresh {
                    0 => Mode::Session,
                    _ => Mode::Refresh,
                },
            })),
        }
    }
}

/// Reads `args` (as `std::env::args().skip(1)` gives them), with `here` the directory to work in
/// when `--cwd` is not given. The error is clap's: it knows how to say itself and how to exit.
pub fn parse(
    args: impl IntoIterator<Item = String>,
    here: PathBuf,
) -> Result<Request, clap::Error> {
    Cli::try_parse_from(args)?.request(here)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    fn run(text: &str, here: &std::path::Path) -> Option<Args> {
        match parse(words(text), here.to_path_buf()) {
            Ok(Request::Run(args)) => Some(args),
            _ => None,
        }
    }

    #[test]
    fn the_desktops_sheet_is_the_default_and_the_terminal_needs_its_flag() {
        let here = PathBuf::from("/home/u/p");
        let plain = run("claude-code", here.as_path()).expect("args");
        assert_eq!(plain.fallback, Fallback::Off);
        assert_eq!(plain.cwd, here);
        assert_eq!(plain.space, SpaceId::desktop());
        let work = run("claude-code --space work", here.as_path()).expect("args");
        assert_eq!(work.space.as_str(), "work");
        let tty = run("claude-code --tty --cwd /home/u/q", here.as_path()).expect("args");
        assert_eq!(tty.fallback, Fallback::Terminal);
        assert_eq!(tty.cwd, PathBuf::from("/home/u/q"));
    }

    #[test]
    fn a_refresh_is_a_flag_and_a_session_is_the_default() {
        let here = PathBuf::from("/home/u/p");
        assert_eq!(
            run("claude-code --refresh", here.as_path()).map(|a| a.mode),
            Some(Mode::Refresh)
        );
        assert_eq!(
            run("claude-code", here.as_path()).map(|a| a.mode),
            Some(Mode::Session)
        );
    }

    #[test]
    fn serve_stands_alone() {
        let here = PathBuf::from("/home/u/p");
        assert_eq!(
            parse(words("--serve"), here.clone()).ok(),
            Some(Request::Serve)
        );
        assert!(parse(words("--serve --tty"), here.clone()).is_err());
        assert!(parse(words("--serve claude-code"), here).is_err());
    }

    #[test]
    fn anything_else_is_a_bad_command_line() {
        let here = PathBuf::from("/home/u/p");
        assert!(parse(Vec::new(), here.clone()).is_err());
        assert!(parse(words("claude-code --yolo"), here.clone()).is_err());
        assert!(parse(words("claude-code --cwd"), here).is_err());
    }
}
