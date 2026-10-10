//! What a store hands back and what names a place: the workspace root, a saved point, the opaque
//! ids of a tree and of one file's content.

use docket_core::{AbsPath, CheckpointId, PathFault, Workspace};
use porter_core::UnixSeconds;

/// The folder a session works in, absolute and normalised: where points are taken and restored.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct WorkRoot(AbsPath);

impl WorkRoot {
    /// `path` as a root.
    pub fn new(path: AbsPath) -> Self {
        Self(path)
    }

    /// The session's workspace as a root; a workspace that steps up with `..` is refused.
    pub fn of(workspace: &Workspace) -> Result<Self, PathFault> {
        AbsPath::parse(workspace.as_str()).map(Self)
    }

    /// The folder.
    pub fn path(&self) -> &AbsPath {
        &self.0
    }
}

/// A saved tree's id. Opaque to everyone but the store that made it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TreeId(String);

impl TreeId {
    /// The id the store minted.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as the store wrote it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The id of one file's content in a tree: equal ids mean equal content. Opaque to everyone
/// but the store that made it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct EntryId(String);

impl EntryId {
    /// The id the store minted.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// The id as the store wrote it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A point the store holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Saved {
    /// Its number in the session.
    pub id: CheckpointId,
    /// When it was taken.
    pub at: UnixSeconds,
    /// What it holds.
    pub tree: TreeId,
}
