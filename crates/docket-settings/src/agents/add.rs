//! Adds an agent from the agent registry to `agents.toml`, and takes an entry out. An added entry
//! names the registry and the pinned version (docket installs it with `docket-agents` and gives it
//! a home of its own), signs in the agent's own way, and reaches the host's network, which it
//! needs to reach its provider: the Settings app says so before it adds one. An entry with a
//! command of its own is the person's to write by hand. The file is edited in place, so comments
//! and the other entries stay.

use super::write::ChoiceFault;
use docket_store::write_atomic;
use std::path::Path;
use toml_edit::{ArrayOfTables, DocumentMut, Item, Table, value};

/// An agent to add from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewAgent {
    /// The name docket lists it under (`program`).
    pub program: String,
    /// The registry's id for it.
    pub registry: String,
    /// The exact version to pin.
    pub version: String,
    /// What the person calls it.
    pub label: String,
}

/// Why an agent was not added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AddFault {
    /// `agents.toml` is not valid TOML, or `agent` in it is not a list of entries.
    #[error("the agents file can't be read")]
    Unreadable,
    /// An entry of that name is listed already.
    #[error("an agent of that name is listed already")]
    Listed,
    /// The name, the registry id or the version has odd characters or is too long.
    #[error("that isn't an agent name docket can keep")]
    BadName,
    /// The label is empty, too long or has control characters.
    #[error("that isn't a name docket can show")]
    BadLabel,
    /// The file could not be written.
    #[error("the agents file can't be written")]
    Io,
}

fn word_fine(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

fn label_fine(label: &str) -> bool {
    !label.trim().is_empty() && label.chars().count() <= 64 && !label.chars().any(char::is_control)
}

fn listed(list: &ArrayOfTables, program: &str) -> bool {
    list.iter()
        .any(|t| t.get("program").and_then(Item::as_str) == Some(program))
}

fn entry_of(agent: &NewAgent) -> Table {
    let mut entry = Table::new();
    entry.insert("program", value(agent.program.as_str()));
    entry.insert("registry", value(agent.registry.as_str()));
    entry.insert("version", value(agent.version.as_str()));
    entry.insert("route", value("login"));
    entry.insert("network", value("host"));
    entry.insert("label", value(agent.label.as_str()));
    entry
}

/// `text` with `agent` listed after the other entries. Empty text is an empty file. Pure.
pub fn edit_agent_add(text: &str, agent: &NewAgent) -> Result<String, AddFault> {
    let names = [&agent.program, &agent.registry, &agent.version];
    if !names.iter().all(|w| word_fine(w)) {
        return Err(AddFault::BadName);
    }
    if !label_fine(&agent.label) {
        return Err(AddFault::BadLabel);
    }
    let mut doc: DocumentMut = text.parse().map_err(|_| AddFault::Unreadable)?;
    let list = doc
        .entry("agent")
        .or_insert(Item::ArrayOfTables(ArrayOfTables::new()))
        .as_array_of_tables_mut()
        .ok_or(AddFault::Unreadable)?;
    if listed(list, &agent.program) {
        return Err(AddFault::Listed);
    }
    list.push(entry_of(agent));
    Ok(doc.to_string())
}

/// `text` without the entry listed as `program`. Pure.
pub fn edit_agent_remove(text: &str, program: &str) -> Result<String, ChoiceFault> {
    let mut doc: DocumentMut = text.parse().map_err(|_| ChoiceFault::Unreadable)?;
    let list = doc
        .get_mut("agent")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or(ChoiceFault::NoSuchAgent)?;
    let before = list.len();
    list.retain(|t| t.get("program").and_then(Item::as_str) != Some(program));
    if list.len() == before {
        return Err(ChoiceFault::NoSuchAgent);
    }
    Ok(doc.to_string())
}

/// Adds `agent` to the file at `path`, which need not exist yet; the file is replaced whole or
/// not at all.
pub fn write_agent_add(path: &Path, agent: &NewAgent) -> Result<(), AddFault> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(_) => return Err(AddFault::Io),
    };
    let edited = edit_agent_add(&text, agent)?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|_| AddFault::Io)?;
    }
    write_atomic(path, edited.as_bytes()).map_err(|_| AddFault::Io)
}

/// Takes the entry of `program` out of the file at `path`; the file is replaced whole or not at
/// all.
pub fn write_agent_remove(path: &Path, program: &str) -> Result<(), ChoiceFault> {
    let text = std::fs::read_to_string(path).map_err(|_| ChoiceFault::Io)?;
    let edited = edit_agent_remove(&text, program)?;
    write_atomic(path, edited.as_bytes()).map_err(|_| ChoiceFault::Io)
}
