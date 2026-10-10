//! A path inside a workspace.

use serde::{Deserialize, Serialize};

/// A path inside the workspace: relative, no `..`, no NUL, no leading `/`.
///
/// Every step is a plain name: an empty step (`a//b`), `.` and `..` are refused, so a path can
/// never name anything outside the folder it is joined to. Reading one from the wire checks the
/// same rules as [`WorkPath::parse`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WorkPath(String);

/// Why text is not a path inside a workspace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WorkPathError {
    /// There is nothing in it.
    #[error("a file name is needed")]
    Empty,
    /// It starts with `/`.
    #[error("a path inside the folder cannot start with /")]
    Absolute,
    /// A step is empty, `.` or `..`.
    #[error("a path inside the folder cannot step sideways or up")]
    Step,
    /// It holds a NUL character.
    #[error("a path cannot hold a NUL character")]
    Nul,
}

impl WorkPath {
    /// `text` as a path inside the workspace.
    pub fn parse(text: &str) -> Result<WorkPath, WorkPathError> {
        if text.is_empty() {
            return Err(WorkPathError::Empty);
        }
        if text.contains('\0') {
            return Err(WorkPathError::Nul);
        }
        if text.starts_with('/') {
            return Err(WorkPathError::Absolute);
        }
        if text.split('/').any(|step| matches!(step, "" | "." | "..")) {
            return Err(WorkPathError::Step);
        }
        Ok(WorkPath(text.to_owned()))
    }

    /// The path, `/`-separated.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl TryFrom<String> for WorkPath {
    type Error = WorkPathError;

    fn try_from(text: String) -> Result<Self, Self::Error> {
        Self::parse(&text)
    }
}

impl From<WorkPath> for String {
    fn from(path: WorkPath) -> String {
        path.0
    }
}
