//! How long a terminal command may run. A command nobody stops would hold its sandbox (and the
//! machine's memory and processes) for ever; past this long it is killed with bulkhead's own
//! kill, and its terminal stays readable like any killed one.

use std::time::Duration;

/// The longest a terminal command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandDeadline(Duration);

impl CommandDeadline {
    /// Ten minutes: long enough for a build or a test run, short enough that a forgotten command
    /// does not live for ever.
    pub const DEFAULT: Self = Self(Duration::from_secs(600));

    /// A deadline of `limit` after a command starts.
    pub const fn new(limit: Duration) -> Self {
        Self(limit)
    }

    /// How long after it starts a command is killed.
    pub const fn limit(self) -> Duration {
        self.0
    }
}

impl Default for CommandDeadline {
    fn default() -> Self {
        Self::DEFAULT
    }
}
