//! What the `Execute` effect is gated as, and why a command may not run in the sandbox
//! (acp-sessions.md section 7, S8).
//!
//! `prov::Effect` is porter's frozen enum and has no `Execute`, so docket gates the effect as
//! [`EXECUTE_AS`], which is `Outbound`: the severity at which untrusted input into the call is
//! never grantable. The router rules a command like any other call (the `Execute` rules of S8
//! are its policy point's, its `may_offer` and its terminal scope: see `docket-router`'s
//! `terminal` and `standing`); this module keeps the two facts the host and the sandbox share.

use prov::Effect;
use serde::{Deserialize, Serialize};

/// The effect class an `Execute` call is gated as.
pub const EXECUTE_AS: Effect = Effect::Outbound;

/// Why a command cannot run in the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum CannotSandbox {
    /// No sandbox program (`bwrap`) is installed.
    #[error("no sandbox program is installed")]
    NotInstalled,
    /// The program is there but the kernel refuses it (user namespaces are off).
    #[error("the kernel refuses unprivileged sandboxes here")]
    NamespacesDenied,
    /// This platform has no sandbox backend.
    #[error("this platform has no sandbox")]
    Unsupported,
    /// The working directory cannot be the one writable place (`/`, or it is not a directory).
    #[error("the working directory cannot be sandboxed")]
    BadWorkingDir,
}

/// Whether this command can run in the sandbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SandboxState {
    /// It can.
    Ready,
    /// It cannot, for this reason.
    Cannot(CannotSandbox),
}
