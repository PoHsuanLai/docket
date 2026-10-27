//! Our own rules for the Agent Client Protocol, and nothing else (design note `acp-sessions.md`).
//! Pure: no connection, no tokio, no file system, no clock, no wording. The wire types are the
//! official `agent-client-protocol` SDK's (`schema` only); the connections are docket-acp's and docket-acp-host's.
//!
//! - `confine`: file calls reach the session's working directory only, links and secrets included,
//!   before any call exists.
//! - `tool_req`, `names`, `call`: an agent's request becomes a formed `AgentCall`, named as an
//!   action of the `org.quire.AcpAgent` pseudo-app.
//! - `court`: the seam through which the router rules on the agent's calls.
//! - `permission`: the gate's ruling on a permission request: only `allow_once` allows.
//! - `taint` and `reported`: what the agent says it did is a display record, trusted for nothing,
//!   except that what it brought in taints the session.
//! - `strikes`: how many refusals end an agent's turn.
//! - `cause`: the step of starting an agent that failed, for the log.
//! - `command_deadline`: how long a terminal command may run.
//! - `fault`: the protocol errors both edges answer with.

pub mod call;
pub mod cause;
pub mod command_deadline;
pub mod confine;
pub mod court;
pub mod fault;
pub mod names;
pub mod permission;
pub mod reported;
pub mod strikes;
pub mod taint;
pub mod tool_req;
