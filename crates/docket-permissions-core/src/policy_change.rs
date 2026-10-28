//! How a new task policy compares with the old one.

use crate::task_policy::Widening;
use serde::{Deserialize, Serialize};

/// How a new policy compares with the old.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum PolicyChange {
    /// Covers less: applied silently.
    Narrows,
    /// Covers the same.
    Same,
    /// Covers more: the person confirms.
    Widens(Vec<Widening>),
}

/// How a policy changed, without its contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyChangeKind {
    /// Covers less.
    Narrows,
    /// Covers the same.
    Same,
    /// Covers more.
    Widens,
}

impl PolicyChange {
    /// The change without its detail.
    pub fn kind(&self) -> PolicyChangeKind {
        match self {
            PolicyChange::Narrows => PolicyChangeKind::Narrows,
            PolicyChange::Same => PolicyChangeKind::Same,
            PolicyChange::Widens(_) => PolicyChangeKind::Widens,
        }
    }
}
