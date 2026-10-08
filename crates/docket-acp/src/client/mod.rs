//! The ACP client edge (design note `acp-sessions.md` section 3b, owner revisions R1 to R3): an
//! external coding agent is a session backend behind our gate.
//!
//! - `AcpBackend`: a `SessionBackend` over a process behind the `Spawn` seam. It speaks ACP as
//!   the client, offers the agent file and terminal methods that run through us, and answers the
//!   agent's permission requests itself.
//! - `Gatekeeper`: the gate in front of the agent's calls: the breaker, one-use approvals, the
//!   agent program's standing grants, and the person. The agent's own "allow" options are never
//!   consulted; its "always" is a standing grant in our store, and it is told `allow_once`.
//! - `confine`: file calls reach the session's working directory only, links and secrets
//!   included. `files`: the file system seam (`OsFiles`).
//! - `reported`: what the agent says it did on its own is recorded as `agent-reported`, trusted
//!   for nothing; its thoughts are never read as instructions.
//! - `fake`: an in-memory pipe, a scripted spawner and the like, for tests.

mod ask;
mod backend;
mod breaker;
mod confine;
pub mod fake;
mod files;
mod gate;
mod handlers;
mod intake;
mod names;
mod reported;
mod rpc;
mod serve;
mod spawn;
mod tool_req;

pub use ask::{AgentAsk, AsTerminal, Ask, Epoch, SHOWN_MAX, Shown, What};
pub use backend::{AcpBackend, Parts, Seams};
pub use breaker::{Breaker, DENIALS_MAX, FLOOD_MAX};
pub use confine::{Care, Confined, Refusal, confine, named};
pub use files::{FileFault, Files, MAX_READ, MAX_WRITE, OsFiles};
pub use gate::{Audit, Basis, Gatekeeper, Ruling, ToolReq, Why};
pub use handlers::UndoNote;
pub use names::{ACP_APP, action, effect, reported_tail, tail};
pub use spawn::{AgentChild, LaunchPlan, Spawn, SpawnFault, Spawned};
