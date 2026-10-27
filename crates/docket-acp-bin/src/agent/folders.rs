//! The folder a coding agent works in under docket's data directory: its own, never the person's
//! home or a project.

use super::{data_dir, env};
use docket_acp_host::Workdir;
use docket_core::AbsPath;
use docket_session::ProgramName;
use std::path::PathBuf;

/// The folder an agent works in when the assistant hands it a turn: its own, under docket's data
/// directory, never the person's home or a project. `None` for a name that is not a plain slug,
/// which cannot be the name of a folder.
fn workspace_name(program: &ProgramName) -> Option<&str> {
    plain_name(program.as_str())
}

/// `name` when it can be the name of a folder: letters, digits, `-`, `_` and `.`, not starting
/// with a dot.
pub fn plain_name(name: &str) -> Option<&str> {
    let plain = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    (plain && !name.starts_with('.')).then_some(name)
}

/// The folder the agent works in, made if it is not there.
pub fn folder(program: &ProgramName) -> Result<String, String> {
    let name = workspace_name(program)
        .ok_or_else(|| "That coding agent can't be used by the assistant.".to_owned())?;
    folder_under("assistant", name)
}

/// The folder `name` under `group` in docket's data directory, made if it is not there, and
/// never the home folder or a place of secrets.
pub fn folder_under(group: &str, name: &str) -> Result<String, String> {
    let dir: PathBuf = data_dir().join(group).join(name);
    std::fs::create_dir_all(&dir).map_err(|_| "The agent's folder can't be made.".to_owned())?;
    let real =
        std::fs::canonicalize(&dir).map_err(|_| "The agent's folder can't be used.".to_owned())?;
    let text = real
        .to_str()
        .ok_or_else(|| "The agent's folder can't be used.".to_owned())?
        .to_owned();
    let path = AbsPath::parse(&text).map_err(|_| "The agent's folder can't be used.".to_owned())?;
    let home = env("HOME").map(PathBuf::from);
    Workdir::resolve(&path, &path, home.as_deref())
        .map_err(|_| "The agent's folder can't be used.".to_owned())?;
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> ProgramName {
        ProgramName::parse(text).expect("program name")
    }

    #[test]
    fn only_a_plain_slug_is_a_folder_name() {
        let rows = [
            ("claude-code", Some("claude-code")),
            ("gemini_cli.2", Some("gemini_cli.2")),
            (".hidden", None),
            ("a/b", None),
            ("..", None),
            ("two words", None),
        ];
        for (text, expected) in rows {
            assert_eq!(workspace_name(&name(text)), expected, "{text}");
        }
    }
}
