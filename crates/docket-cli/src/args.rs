//! The command line, as words. This stage knows nothing of any app: it splits the arguments
//! into the command, the global flags and the raw `--param value` pairs. Which app, action and
//! parameters exist is `resolve`'s and `params`'s business, from the manifests.

use crate::exit::Failure;
use crate::sessions::SessionsCmd;
use prov::{SessionId, SpaceId};
use std::collections::BTreeSet;

/// Whether the person asked for JSON; without it, JSON is chosen when stdout is not a terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JsonFlag {
    /// `--json` was given.
    Asked,
    /// It was not: the terminal decides.
    Auto,
}

/// Whether to run the call or only describe what it would change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    /// Perform it.
    Run,
    /// `--dry-run`: print the preview.
    DryRun,
}

/// Which undo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoWhich {
    /// This journal row.
    Entry(u64),
    /// `--last`: the newest row of the terminal's own.
    Last,
}

/// One call, as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallArgs {
    /// The app word (a slug or a full name).
    pub app: String,
    /// The action word (without the app's prefix, or the full name).
    pub action: String,
    /// Run or dry run.
    pub mode: RunMode,
    /// `--session`.
    pub session: Option<SessionId>,
    /// `--<param> <value>`, in order, the flag spelled with underscores.
    pub params: Vec<(String, String)>,
    /// What the action acts on.
    pub targets: Vec<String>,
}

/// What was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// `--help`, or nothing at all.
    Help,
    /// `--version`.
    Version,
    /// `apps`.
    Apps,
    /// `skills`.
    Skills,
    /// `<app> --list`.
    List {
        /// The app word.
        app: String,
    },
    /// `describe <app> <action>`.
    Describe {
        /// The app word.
        app: String,
        /// The action word.
        action: String,
    },
    /// `<app> search <text>`.
    Search {
        /// The app word.
        app: String,
        /// The words to find.
        text: String,
    },
    /// `<app> context`.
    Context {
        /// The app word.
        app: String,
    },
    /// `undo <id>` or `undo --last`.
    Undo(UndoWhich),
    /// `ask [--space <id>] <text>`: talk to the companion.
    Ask {
        /// What the person says.
        text: String,
        /// The Space the conversation lives in (default `desktop`).
        space: SpaceId,
    },
    /// `sessions`, `sessions load <id>`, `sessions fork <id> [--at <row>]`: the stored sessions
    /// the terminal may bring back.
    Sessions(SessionsCmd),
    /// `<app> <action> …`.
    Call(CallArgs),
    /// `__complete <words…>`: what a shell completes next (the completion files call it).
    Complete(Vec<String>),
}

/// The whole command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    /// What was asked.
    pub command: Command,
    /// `--json`.
    pub json: JsonFlag,
}

/// The flags that take no value.
const SWITCHES: [&str; 6] = ["json", "dry-run", "list", "last", "help", "version"];

#[derive(Default)]
struct Scan {
    positional: Vec<String>,
    params: Vec<(String, String)>,
    /// Which of [`SWITCHES`] were given.
    switches: BTreeSet<&'static str>,
    session: Option<String>,
    space: Option<String>,
}

impl Scan {
    fn has(&self, switch: &str) -> bool {
        self.switches.contains(switch)
    }
}

fn scan(words: &[String]) -> Result<Scan, Failure> {
    let mut out = Scan::default();
    let mut iter = words.iter();
    while let Some(word) = iter.next() {
        let Some(flag) = word.strip_prefix("--").filter(|f| !f.is_empty()) else {
            match word.as_str() {
                "-h" => {
                    out.switches.insert("help");
                }
                "--" => out.positional.extend(iter.by_ref().cloned()),
                short if short.len() > 1 && short.starts_with('-') => {
                    return Err(Failure::usage(format!("unknown option {short}")));
                }
                _ => out.positional.push(word.clone()),
            }
            continue;
        };
        let (name, inline) = match flag.split_once('=') {
            Some((name, value)) => (name, Some(value.to_owned())),
            None => (flag, None),
        };
        let name = name.replace('_', "-");
        match SWITCHES.iter().find(|s| **s == name) {
            Some(switch) => {
                out.switches.insert(switch);
            }
            None => {
                let other = name.as_str();
                let value = match inline {
                    Some(value) => value,
                    None => iter
                        .next()
                        .cloned()
                        .ok_or_else(|| Failure::usage(format!("--{other} needs a value")))?,
                };
                if other == "session" {
                    out.session = Some(value);
                } else if other == "space" {
                    out.space = Some(value);
                } else {
                    out.params.push((other.replace('-', "_"), value));
                }
            }
        }
    }
    Ok(out)
}

fn session_word(word: &str) -> Result<SessionId, Failure> {
    SessionId::parse(word).map_err(|_| Failure::usage(format!("{word:?} is not a session id")))
}

fn command(scan: &Scan) -> Result<Command, Failure> {
    let words: Vec<&str> = scan.positional.iter().map(String::as_str).collect();
    let no_flags = |what: &str| -> Result<(), Failure> {
        if scan.params.is_empty() {
            Ok(())
        } else {
            Err(Failure::usage(format!("{what} takes no parameters")))
        }
    };
    if scan.has("version") {
        return Ok(Command::Version);
    }
    match words.as_slice() {
        _ if scan.has("help") => Ok(Command::Help),
        [] if scan.has("last") => Ok(Command::Undo(UndoWhich::Last)),
        [] => Err(Failure::usage("no command given (try: quire-do --help)")),
        ["apps"] => no_flags("apps").map(|()| Command::Apps),
        ["apps", ..] => Err(Failure::usage("apps takes no arguments")),
        ["skills"] => no_flags("skills").map(|()| Command::Skills),
        ["skills", ..] => Err(Failure::usage("skills takes no arguments")),
        ["describe", app, action] => Ok(Command::Describe {
            app: (*app).to_owned(),
            action: (*action).to_owned(),
        }),
        ["describe", ..] => Err(Failure::usage("usage: quire-do describe <app> <action>")),
        ["ask", text @ ..] if !text.is_empty() => no_flags("ask").and_then(|()| {
            Ok(Command::Ask {
                text: text.join(" "),
                space: match scan.space.as_deref() {
                    None => SpaceId::desktop(),
                    Some(id) => SpaceId::parse(id)
                        .map_err(|_| Failure::usage("--space is not a Space id"))?,
                },
            })
        }),
        ["ask"] => Err(Failure::usage("usage: quire-do ask <text>")),
        ["sessions"] => no_flags("sessions").map(|()| Command::Sessions(SessionsCmd::List)),
        ["sessions", "load", id] => no_flags("sessions load")
            .and_then(|()| session_word(id))
            .map(|id| Command::Sessions(SessionsCmd::Load(id))),
        ["sessions", "fork", id] => {
            let at = match scan.params.as_slice() {
                [] => None,
                [(name, row)] if name == "at" => Some(
                    row.parse::<u64>()
                        .map_err(|_| Failure::usage("--at is not a row number"))?,
                ),
                _ => {
                    return Err(Failure::usage(
                        "usage: quire-do sessions fork <id> [--at <row>]",
                    ));
                }
            };
            Ok(Command::Sessions(SessionsCmd::Fork {
                session: session_word(id)?,
                at,
            }))
        }
        ["sessions", ..] => Err(Failure::usage(
            "usage: quire-do sessions | sessions load <id> | sessions fork <id> [--at <row>]",
        )),
        ["undo"] if scan.has("last") => Ok(Command::Undo(UndoWhich::Last)),
        ["undo", id] => id
            .parse::<u64>()
            .map(|n| Command::Undo(UndoWhich::Entry(n)))
            .map_err(|_| Failure::usage(format!("{id:?} is not an undo id"))),
        ["undo", ..] => Err(Failure::usage("usage: quire-do undo <undo-id> | --last")),
        [app] if scan.has("list") => Ok(Command::List {
            app: (*app).to_owned(),
        }),
        [app, "search", text @ ..] if !text.is_empty() => Ok(Command::Search {
            app: (*app).to_owned(),
            text: text.join(" "),
        }),
        [_, "search"] => Err(Failure::usage("usage: quire-do <app> search <text>")),
        [app, "context"] => no_flags("context").map(|()| Command::Context {
            app: (*app).to_owned(),
        }),
        [_] => Err(Failure::usage(
            "usage: quire-do <app> <action> …, or quire-do <app> --list",
        )),
        [app, action, targets @ ..] => Ok(Command::Call(CallArgs {
            app: (*app).to_owned(),
            action: (*action).to_owned(),
            mode: if scan.has("dry-run") {
                RunMode::DryRun
            } else {
                RunMode::Run
            },
            session: scan
                .session
                .as_deref()
                .map(SessionId::parse)
                .transpose()
                .map_err(|_| Failure::usage("--session is not a session id"))?,
            params: scan.params.clone(),
            targets: targets.iter().map(|t| (*t).to_owned()).collect(),
        })),
    }
}

/// Reads the command line (without the program name).
pub fn parse(words: &[String]) -> Result<Parsed, Failure> {
    // A shell's half-typed words (`--`, `-`, `--to=`) are not a command line: they pass as typed.
    if let Some((first, typed)) = words.split_first()
        && first == "__complete"
    {
        return Ok(Parsed {
            command: Command::Complete(typed.to_vec()),
            json: JsonFlag::Auto,
        });
    }
    let scan = scan(words)?;
    let json = if scan.has("json") {
        JsonFlag::Asked
    } else {
        JsonFlag::Auto
    };
    Ok(Parsed {
        command: command(&scan)?,
        json,
    })
}
