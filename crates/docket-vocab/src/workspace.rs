//! The directory an editor opened a session in.

use serde::{Deserialize, Serialize};

/// The absolute directory an editor opened a session in (ACP `cwd`): 1 to 4096 characters, starts
/// with `/`, no control characters. It scopes the session; it grants nothing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Workspace(String);

/// Why text is not a workspace.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a workspace is an absolute path of 1 to 4096 characters with no control characters")]
pub struct WorkspaceError;

impl Workspace {
    /// `text` as a workspace.
    pub fn parse(text: &str) -> Result<Self, WorkspaceError> {
        if text.starts_with('/') && text.len() <= 4096 && !text.chars().any(char::is_control) {
            Ok(Self(text.to_owned()))
        } else {
            Err(WorkspaceError)
        }
    }

    /// The path.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for Workspace {
    type Error = WorkspaceError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<Workspace> for String {
    fn from(w: Workspace) -> String {
        w.0
    }
}
