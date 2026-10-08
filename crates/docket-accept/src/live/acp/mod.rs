//! The ACP engine of `docket-live`: an external agent (Claude Code through its ACP adapter, say)
//! in the place of companiond's planner. The same scratch world (a private bus, scratch HOME and
//! XDG directories, the fake mail, a scripted person answering the sheets), the same flows and
//! the same outcome checks; the agent is hosted exactly as `docket-agent` hosts it, through the
//! router, in its sandbox, with the per-session tool edge offered.
//!
//! - `spec`: the agent, as an `agents.toml` entry from the command line.
//! - `secret`: the credentials of a `login` agent (staged for the run, removed after it, and
//!   scrubbed from everything the harness writes).
//! - `play`: one turn of the person's words on the hosted agent.
//! - `run`: a flow played that way, judged by `flows::judge_in` with the checks that look inside
//!   the planner marked not applicable.

pub mod play;
pub mod run;
pub mod secret;
pub mod spec;

pub use play::{AgentEnd, Played, play};
pub use run::{AcpFlowReport, agent_cassette, run_flow_acp};
pub use secret::{Credentials, Redactor};
pub use spec::{AcpSpec, CredentialsSource, SpecFault};
