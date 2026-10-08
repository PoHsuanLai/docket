//! The seam that starts an external agent and hands back its stdio as a `Wire`. The real one is
//! `docket-launch`'s (accounts, sandbox, process); `fake::FakeSpawn` returns an in-memory pipe.

use super::edge::EdgeBind;
use crate::wire::Wire;
use docket_core::AbsPath;
use docket_session::ProgramName;
use prov::SessionId;
use std::future::Future;

/// What to start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LaunchPlan {
    /// The configured program.
    pub program: ProgramName,
    /// The session it runs for (the launcher maps it to its own session grammar).
    pub session: SessionId,
    /// The directory it works in, and the only place it may write.
    pub cwd: AbsPath,
    /// The tool edge of its session, when one is offered: what the sandbox must bind in so the
    /// agent can start the bridge program and reach its socket.
    pub edge: Option<EdgeBind>,
}

/// Why an agent did not start.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SpawnFault {
    /// Agent programs are off, or this one is not configured.
    #[error("agent programs are off or this one is not configured")]
    NotAllowed,
    /// The account service refused or is not there.
    #[error("the account service did not give the agent what it needs")]
    Accounts,
    /// The sandbox could not be made.
    #[error("the sandbox could not be made")]
    Sandbox,
    /// The process did not start.
    #[error("the process did not start")]
    Process,
}

/// Extra `_meta` for `session/new`, the launcher's: an agent-specific request the host writes
/// (never the agent) and sends as it opens the session. The client does not read it.
pub type SessionMeta = serde_json::Map<String, serde_json::Value>;

/// The way of signing in the person chose for an agent: one of the ids the agent advertises
/// when it starts. Written in `agents.toml`, never taken from the agent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignIn(String);

/// The longest sign-in id, in characters.
const SIGN_IN_MAX: usize = 64;

/// Why a sign-in id was not accepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("a sign-in method is 1 to 64 letters, digits, - _ or .")]
pub struct SignInRefused;

impl SignIn {
    /// A short id: letters, digits, `-`, `_` and `.`.
    pub fn parse(text: &str) -> Result<Self, SignInRefused> {
        let fine = !text.is_empty()
            && text.len() <= SIGN_IN_MAX
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
        if fine {
            Ok(Self(text.to_owned()))
        } else {
            Err(SignInRefused)
        }
    }

    /// The id as the agent knows it.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A started agent: its stdio and the handle that ends it.
#[derive(Debug)]
pub struct Spawned<W, C> {
    /// Its stdio, a line each way.
    pub wire: W,
    /// What ends it.
    pub child: C,
    /// The `_meta` of `session/new`, when the launcher has any (a preset in `agents.toml`).
    pub meta: Option<SessionMeta>,
    /// The way to sign in before `session/new`, when `agents.toml` names one.
    pub sign_in: Option<SignIn>,
}

/// The process behind a wire.
pub trait AgentChild: Send {
    /// Ends the process now. Safe to repeat.
    fn kill(&mut self);

    /// Ends the process and gives back everything it was lent: the model endpoint, the
    /// session at the account service, a credential. Safe to repeat.
    fn close(&mut self) -> impl Future<Output = ()> + Send;
}

/// Starts agents.
pub trait Spawn: Send {
    /// Its stdio.
    type Wire: Wire;
    /// Its handle.
    type Child: AgentChild;

    /// Starts the agent for `plan`.
    fn spawn(
        &mut self,
        plan: &LaunchPlan,
    ) -> impl Future<Output = Result<Spawned<Self::Wire, Self::Child>, SpawnFault>> + Send;
}
