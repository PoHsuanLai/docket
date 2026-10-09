//! Writes the person's choices into `agents.toml`: the model and the way of signing in of one
//! entry, and nothing else. The file is edited in place, so comments and every other field stay.

use docket_core::write_atomic;
use std::path::Path;
use toml_edit::{DocumentMut, Item, value};

/// What to do with one field of the entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pick {
    /// Leave it as it is.
    Keep,
    /// Set it to this id.
    Set(String),
    /// Remove it: the agent chooses.
    Clear,
}

/// The choices for one agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentChoice {
    /// The model, by the id the agent gives it.
    pub model: Pick,
    /// The way of signing in, by the id the agent gives it.
    pub way: Pick,
}

/// Why the choice was not written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ChoiceFault {
    /// `agents.toml` is not valid TOML.
    #[error("the agents file can't be read")]
    Unreadable,
    /// No such agent is listed.
    #[error("that agent isn't listed")]
    NoSuchAgent,
    /// The model id has spaces or is too long.
    #[error("that isn't a model name docket can keep")]
    BadModel,
    /// The sign-in id has odd characters or is too long.
    #[error("that isn't a way of signing in docket can keep")]
    BadWay,
    /// The file could not be written.
    #[error("the agents file can't be written")]
    Io,
}

fn model_fine(id: &str) -> bool {
    !id.is_empty()
        && id.chars().count() <= 128
        && !id.chars().any(|c| c.is_whitespace() || c.is_control())
}

fn way_fine(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn apply(entry: &mut toml_edit::Table, key: &str, pick: &Pick) {
    match pick {
        Pick::Keep => {}
        Pick::Set(id) => {
            entry.insert(key, value(id.as_str()));
        }
        Pick::Clear => {
            entry.remove(key);
        }
    }
}

/// `text` with the choices set in the entry listed as `program`. Pure.
pub fn edit_agent_choice(
    text: &str,
    program: &str,
    choice: &AgentChoice,
) -> Result<String, ChoiceFault> {
    if matches!(&choice.model, Pick::Set(id) if !model_fine(id)) {
        return Err(ChoiceFault::BadModel);
    }
    if matches!(&choice.way, Pick::Set(id) if !way_fine(id)) {
        return Err(ChoiceFault::BadWay);
    }
    let mut doc: DocumentMut = text.parse().map_err(|_| ChoiceFault::Unreadable)?;
    let entry = doc
        .get_mut("agent")
        .and_then(Item::as_array_of_tables_mut)
        .and_then(|list| {
            list.iter_mut()
                .find(|t| t.get("program").and_then(Item::as_str) == Some(program))
        })
        .ok_or(ChoiceFault::NoSuchAgent)?;
    apply(entry, "model", &choice.model);
    apply(entry, "sign_in", &choice.way);
    Ok(doc.to_string())
}

/// Sets the choices in the entry of `program` in the file at `path`; the file is replaced whole
/// or not at all.
pub fn write_agent_choice(
    path: &Path,
    program: &str,
    choice: &AgentChoice,
) -> Result<(), ChoiceFault> {
    let text = std::fs::read_to_string(path).map_err(|_| ChoiceFault::Io)?;
    let edited = edit_agent_choice(&text, program, choice)?;
    write_atomic(path, edited.as_bytes()).map_err(|_| ChoiceFault::Io)
}
