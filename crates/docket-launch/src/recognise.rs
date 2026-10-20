//! The inverse of [`Entry::resume_line`]: from the command line a terminal saw in a pane, which
//! listed app it is picking a conversation up of, and which conversation. The terminal cannot
//! read `agents.toml`; the host that can answers for it.

use crate::config::{AgentsFile, Entry, SESSION_MARK};
use docket_core::{AppSessionId, SeenCommand, WatchedAgent};
use std::path::Path;

/// How one word of a command line stands to one argument of a resume template.
enum Match<'a> {
    /// The word is the argument, written out.
    Same,
    /// The word is the argument with the conversation's id where `{session}` stands.
    Id(&'a str),
}

/// How `word` stands to the template argument `arg`, or none when it is not that argument.
fn match_arg<'a>(arg: &str, word: &'a str) -> Option<Match<'a>> {
    match arg.split_once(SESSION_MARK) {
        None => (arg == word).then_some(Match::Same),
        Some((before, after)) => word
            .strip_prefix(before)
            .and_then(|rest| rest.strip_suffix(after))
            .map(Match::Id),
    }
}

/// The id `window` carries when it is the template's arguments with an id where `{session}`
/// stands.
fn id_in<'a>(template: &[String], window: &'a [String]) -> Option<&'a str> {
    let mut found = None;
    for (arg, word) in template.iter().zip(window) {
        if let Match::Id(id) = match_arg(arg, word)? {
            found = found.or(Some(id));
        }
    }
    found
}

/// Whether `word` names the entry's program: its path, or any path that ends in the same file
/// name (a wrapper that found it on the `PATH`).
fn names_command(entry: &Entry, word: &str) -> bool {
    let command = entry.command.as_str();
    let file_name = |path: &str| Path::new(path).file_name().map(|name| name.to_owned());
    word == command || file_name(word).is_some_and(|name| Some(name) == file_name(command))
}

/// The conversation `words` pick up in `entry`'s program, when they do.
fn recognise_entry(entry: &Entry, words: &[String]) -> Option<WatchedAgent> {
    let template = entry.resume.as_deref().filter(|args| !args.is_empty())?;
    words
        .iter()
        .enumerate()
        .filter(|(_, word)| names_command(entry, word))
        .find_map(|(at, _)| {
            words[at + 1..]
                .windows(template.len())
                .find_map(|window| id_in(template, window))
        })
        .and_then(|id| AppSessionId::parse(id).ok())
        .map(|session| WatchedAgent {
            program: entry.name.clone(),
            session,
        })
}

impl AgentsFile {
    /// The app and the conversation `seen` is the app's own program picking up, from the first
    /// listed app that has resume arguments and whose command and arguments the line holds; none
    /// when it is no listed app, has no such arguments, or the id is not a plain word.
    pub fn recognise(&self, seen: &SeenCommand) -> Option<WatchedAgent> {
        self.entries()
            .find_map(|entry| recognise_entry(entry, seen.tokens()))
    }
}
