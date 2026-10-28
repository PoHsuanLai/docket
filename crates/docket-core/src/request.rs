//! What a caller asks the router for, and where the call came from.

use crate::ids::{ActionRef, Handle};
use crate::value::{Args, TargetValue, Value};
use serde::{Deserialize, Serialize};

/// Where a call came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    /// The launcher.
    Launcher,
    /// A prompt field inside an app's window.
    InWindowField,
    /// The companion.
    Companion,
    /// A keyboard shortcut.
    Shortcut,
    /// An external MCP client.
    Mcp,
    /// A process running `quire-do`.
    Cli,
    /// An app calling itself.
    AppInternal,
    /// The companion's read of memory at the start of a turn the person typed, for the words
    /// they typed. The router honours it for one action only ([`RECALL_APP`], [`RECALL_ACTION`]),
    /// for the main planner, on a turn recorded from a launcher, a field or an editor; any other
    /// use is refused, never asked.
    TurnStart,
}

/// The app whose search the start of a person's turn may run unasked.
pub const RECALL_APP: &str = "org.quire.Memory";
/// The one action of [`RECALL_APP`] a turn start may run unasked.
pub const RECALL_ACTION: &str = "memory.recall";

/// What a caller asks the router for (the app is in `action`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallRequest {
    /// Which action.
    pub action: ActionRef,
    /// What it acts on.
    pub target: TargetValue,
    /// Its arguments, with provenance.
    pub args: Args,
    /// Where the call came from.
    pub origin: Origin,
}

impl CallRequest {
    /// The handles the call names: its target first, then its arguments in order (a handle
    /// inside a list or a record counts). A request the router has already resolved names none.
    pub fn handles(&self) -> Vec<Handle> {
        fn walk(value: &Value, into: &mut Vec<Handle>) {
            match value {
                Value::Handle(h) => into.push(*h),
                Value::List(items) => items.iter().for_each(|i| walk(i, into)),
                Value::Record(fields) => fields.values().for_each(|v| walk(v, into)),
                _ => {}
            }
        }
        let mut found = match &self.target {
            TargetValue::Handles(hs) => hs.clone(),
            _ => Vec::new(),
        };
        self.args.values().for_each(|a| walk(&a.value, &mut found));
        found
    }
}
