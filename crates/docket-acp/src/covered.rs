//! The actions this editor's person said "allow always" to on this connection. It lets the
//! editor's own extra prompt (mode `ask`) stand down for them, nothing more: the grant itself
//! lives in the router, which still asks, by a sheet, for any call the grant does not cover
//! (another path, another recipient, a revoked grant) and runs every review on the ones it does.

use docket_core::ActionRef;
use std::collections::BTreeSet;

/// Actions the person chose "allow always" for, by name only; the scope is the router's.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Covered(BTreeSet<ActionRef>);

impl Covered {
    /// Whether the person already said "always" to `action`.
    pub fn holds(&self, action: &ActionRef) -> bool {
        self.0.contains(action)
    }

    /// The same, and `action` too.
    #[must_use]
    pub fn with(mut self, action: ActionRef) -> Self {
        self.0.insert(action);
        self
    }
}
