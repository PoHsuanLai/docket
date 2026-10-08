//! The ACP client edge (design note `acp-sessions.md` section 3b, owner revisions R1 to R3): an
//! external coding agent is a session backend whose every call goes through the router.
//!
//! - `AcpBackend`: a `SessionBackend` over a process behind the `Spawn` seam. It speaks ACP as
//!   the client, offers the agent file and terminal methods, and answers the agent's permission
//!   requests itself, each from the router's ruling and never from the agent's own options.
//! - `Court`: the router as the agent's host calls it. `AgentCall` is what the agent asked,
//!   formed and confined; `Ruled` is how the router ruled. `IntentsCourt` is the real one, over
//!   `docket-client`. The host decides nothing: not whether to ask the person, not whether a
//!   standing grant stands in, not whether the breaker is tripped. Those are the router's, and
//!   the sheet is whatever the router's confirmer is (sill's, or the host's own in the
//!   development fallback).
//! - `Performer`: what happens after the router said yes (read the file, write it, start the
//!   command in the sandbox). Nothing in it runs unless the gate allowed the call; each call
//!   carries the handle (`StageId`) of the request the host formed for it.
//! - `confine`: file calls reach the session's working directory only, links and secrets
//!   included, before any call exists. `files`: the file system seam (`OsFiles`).
//! - `reported`: what the agent says it did on its own is a display record, trusted for
//!   nothing, except that what it brought in taints the session (`taint`); its thoughts are
//!   never read as instructions.
//! - `host`: `AgentHost`, the `SessionHost` that opens the agent's session at the router,
//!   records the person's turns there, and hands the router's sheets to the person.
//! - `fake`: an in-memory pipe, a scripted spawner and the like, for tests.

mod backend;
mod confine;
mod court;
pub mod fake;
mod files;
mod handlers;
mod host;
mod intake;
mod intents_court;
mod names;
mod performer;
mod reported;
mod rpc;
mod serve;
mod spawn;
mod strikes;
mod taint;
mod tool_req;

pub use backend::{AcpBackend, Parts, Seams};
pub use confine::{Care, Confined, Refusal, confine, named};
pub use court::{AgentCall, Command, Court, CourtFault, OpenAgent, PermissionAsk, Ruled, StageId};
pub use files::{FileFault, Files, MAX_READ, MAX_WRITE, OsFiles};
pub use host::{AgentHost, Fallback};
pub use intents_court::IntentsCourt;
pub use names::{effect, permission, reported_action};
pub use performer::{Performer, UndoNote};
pub use spawn::{AgentChild, LaunchPlan, Spawn, SpawnFault, Spawned};
pub use strikes::{STRIKES_MAX, Strikes};
pub use taint::{TaintSource, brings_content};
pub use tool_req::{Asked, tool_req};
