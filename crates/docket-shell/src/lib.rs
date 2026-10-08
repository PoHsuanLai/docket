//! The sandboxed shell tool (acp-sessions.md S8).
//!
//! - `Sandbox` and `Job`: the seam. `BwrapSandbox` runs a command under bubblewrap (a separate
//!   process, read-only root, write only to the working directory, no network, cleared
//!   environment); `FakeSandbox` records what it was asked and answers from a script.
//! - `Shell`: the terminal table over a sandbox. It never runs a command unsandboxed; when the
//!   sandbox cannot confine one it refuses with the reason.
//! - `redact`, `Tail`, `shown`: output is bounded and secrets are masked before anyone reads it.
//! - `sandbox_env`: the environment is cleared and rebuilt from an allowlist.
//!
//! Whether a command may run at all is the gate's decision (`docket_core::rule_execute`); this
//! crate only runs what the gate let through, and only inside the sandbox.

mod bwrap;
mod bwrap_job;
mod env;
pub mod fake;
mod output;
mod redact;
mod sandbox;
mod tool;

pub use bwrap::{BwrapSandbox, Detected, HIDDEN, bwrap_args};
pub use bwrap_job::BwrapJob;
pub use env::{SANDBOX_HOME, SANDBOX_PATH, dropped, sandbox_env};
pub use output::{Shown, Tail, shown};
pub use redact::{MASK, redact};
pub use sandbox::{
    Argv, ByteLimit, Captured, Cut, EnvVar, ExitReport, Job, KILLED, Network, RunSpec, Sandbox,
    StartFault,
};
pub use tool::{
    DEFAULT_KEEP, INLINE_MAX, Launch, MAX_KEEP, MAX_TERMINALS, Shell, ShellFault, Snapshot, TermId,
    View,
};
