//! Whether the edge is on. MCP is off by default (QUESTIONS S7, actions.md section 3.14): an
//! edge that was never switched on lists no tools and refuses every call.
//!
//! The switch belongs to the person's settings (the Intelligence page); the daemon that hosts
//! the edge reads it and hands the edge the value. Nothing here reads a file or the environment.

use serde::{Deserialize, Serialize};

/// Whether external clients may use the registry's offered actions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpAccess {
    /// No tools are listed and no call is made (the default).
    #[default]
    Off,
    /// The offered actions are tools.
    On,
}
