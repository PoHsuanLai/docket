//! The command line of `docket-agent`: `docket-agent PROGRAM [--cwd DIR] [--space NAME] [--tty]`,
//! or `docket-agent PROGRAM --refresh`: start the agent briefly, write down what it offers, and
//! stop it without a turn.

use docket_acp::client::Fallback;
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

/// Reads `args` (the program's name first, as `std::env::args().skip(1)` gives them), with
/// `here` the directory to work in when `--cwd` is not given. `None` for anything else.
pub fn parse(args: impl IntoIterator<Item = String>, here: PathBuf) -> Option<Args> {
    let mut it = args.into_iter();
    let program = ProgramName::parse(&it.next()?).ok()?;
    let mut cwd = here;
    let mut fallback = Fallback::Off;
    let mut space = SpaceId::desktop();
    let mut mode = Mode::Session;
    while let Some(flag) = it.next() {
        match flag.as_str() {
            "--cwd" => cwd = PathBuf::from(it.next()?),
            "--space" => space = SpaceId::parse(&it.next()?).ok()?,
            "--tty" => fallback = Fallback::Terminal,
            "--refresh" => mode = Mode::Refresh,
            _ => return None,
        }
    }
    Some(Args {
        program,
        cwd,
        space,
        fallback,
        mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        text.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn the_desktops_sheet_is_the_default_and_the_terminal_needs_its_flag() {
        let here = PathBuf::from("/home/u/p");
        let plain = parse(words("claude-code"), here.clone()).expect("args");
        assert_eq!(plain.fallback, Fallback::Off);
        assert_eq!(plain.cwd, here);
        assert_eq!(plain.space, SpaceId::desktop());
        let work = parse(words("claude-code --space work"), here.clone()).expect("args");
        assert_eq!(work.space.as_str(), "work");
        let tty = parse(words("claude-code --tty --cwd /home/u/q"), here.clone()).expect("args");
        assert_eq!(tty.fallback, Fallback::Terminal);
        assert_eq!(tty.cwd, PathBuf::from("/home/u/q"));
    }

    #[test]
    fn a_refresh_is_a_flag_and_a_session_is_the_default() {
        let here = PathBuf::from("/home/u/p");
        assert_eq!(
            parse(words("claude-code --refresh"), here.clone()).map(|a| a.mode),
            Some(Mode::Refresh)
        );
        assert_eq!(
            parse(words("claude-code"), here).map(|a| a.mode),
            Some(Mode::Session)
        );
    }

    #[test]
    fn anything_else_is_a_bad_command_line() {
        let here = PathBuf::from("/home/u/p");
        assert!(parse(Vec::new(), here.clone()).is_none());
        assert!(parse(words("claude-code --yolo"), here.clone()).is_none());
        assert!(parse(words("claude-code --cwd"), here).is_none());
    }
}
