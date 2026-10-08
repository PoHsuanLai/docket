//! The seam that starts an external agent and hands back its stdio as a `Wire`. The real one is
//! `docket-launch`'s (accounts, sandbox, process); `fake::FakeSpawn` returns an in-memory pipe.

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

/// A started agent: its stdio and the handle that ends it.
#[derive(Debug)]
pub struct Spawned<W, C> {
    /// Its stdio, a line each way.
    pub wire: W,
    /// What ends it.
    pub child: C,
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
