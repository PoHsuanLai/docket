//! How far one ask may go.

use porter_core::Count;

/// The brakes on one ask. The router's own budgets (calls, writes, outbound, per minute) apply
/// beside them and are not set here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most model turns before the ask ends failed (the planner's reply counts as one turn,
    /// whatever it called).
    pub steps: Count,
}

impl Default for Limits {
    fn default() -> Self {
        Self { steps: Count(8) }
    }
}
