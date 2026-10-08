//! Starts the external coding agents that `docket-acp`'s client edge drives (design note
//! `acp-sessions.md` section 3b, owner revisions R2, R3). The pieces:
//!
//! - `config`: `agents.toml`, the programs the person lists; off unless `agent.acp.agents` is on
//!   (`permit`).
//! - `accounts`: the seam to porter (accountd's launcher interface, inferd's agent endpoints);
//!   `dbus::DbusAccounts` is the real link, `fake::FakeAccounts` the test one.
//! - `spawn`: `AgentSpawn`, the real `Spawn`: a launcher session, the model route (an inferd
//!   endpoint, a handed-off key, or the agent's own sign-in), the environment built from nothing
//!   (`env`), the sandbox (`procs`, bubblewrap) with its network mode, and the duties that give
//!   everything back when the session closes or the process ends.
//! - `supervise`: registering the programs, ending a process whose credential accountd revoked,
//!   and running the agent's own login visibly (`login`) when asked.
//!
//! docket never reads an agent's token: a login runs the agent's own command and reports only its
//! coarse outcome.

pub mod accounts;
pub mod config;
#[cfg(feature = "dbus")]
pub mod dbus;
pub mod env;
pub mod fake;
pub mod login;
mod login_only;
pub mod managed;
mod names;
mod permit;
pub mod procs;
pub mod spawn;
pub mod supervise;

pub use accounts::{
    AccountFault, Accounts, AskKind, Heard, Issued, KeyHandoff, LoginAsk, OpenedEndpoint, RouteWish,
};
pub use config::{
    AgentsFile, ConfigFault, Delivery, Endpoint, EndpointKind, Entry, Profile, Route, ToolsMode,
};
pub use login_only::LoginOnly;
pub use names::launcher_session;
pub use permit::{AgentsPermit, Refusal};
pub use procs::{BwrapProcs, ChildProc, Proc, ProcFault, Procs, StdioWire};
pub use spawn::{AgentSpawn, Child, Registry};
pub use supervise::Supervisor;
