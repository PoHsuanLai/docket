//! The launcher's long-running duties: register the programs, then listen. A revocation ends the
//! process that holds the credential (accountd cannot reach the child's copy of a key). A login
//! or logout request runs the agent's own command visibly and reports only the coarse outcome.

use crate::accounts::{Accounts, Heard};
use crate::config::AgentsFile;
use crate::login::LoginRunner;
use crate::procs::Proc;
use crate::spawn::Registry;
use std::sync::Arc;

/// Listens for what porter says to this launcher.
pub struct Supervisor<A, P, L> {
    accounts: Arc<A>,
    file: Arc<AgentsFile>,
    registry: Registry<P>,
    login: L,
}

impl<A, P, L> std::fmt::Debug for Supervisor<A, P, L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor").finish_non_exhaustive()
    }
}

impl<A: Accounts, P: Proc, L: LoginRunner> Supervisor<A, P, L> {
    /// A supervisor over the shared link and registry.
    pub fn new(accounts: Arc<A>, file: Arc<AgentsFile>, registry: Registry<P>, login: L) -> Self {
        Self {
            accounts,
            file,
            registry,
            login,
        }
    }

    /// Registers the programs. A program another launcher holds is a fault, and none is taken.
    pub async fn register(&self) -> Result<(), crate::accounts::AccountFault> {
        self.accounts.register(&self.file.programs()).await
    }

    /// Handles one thing porter said. False when the link is gone.
    pub async fn step(&self) -> bool {
        let Some(heard) = self.accounts.hear().await else {
            return false;
        };
        match heard {
            Heard::Revoked(id) => {
                self.registry.kill(&id);
            }
            Heard::Ask(ask) => {
                // A program we did not list gets no command: the outcome says it is not here.
                let outcome = match self.file.by_program(&ask.program) {
                    Some(entry) => self.login.run(entry, ask.kind).await,
                    None => {
                        porter_core::LoginOutcome::Failed(porter_core::LoginFault::NotInstalled)
                    }
                };
                let _ = self.accounts.report(&ask, outcome).await;
            }
        }
        true
    }

    /// Listens until the link ends.
    pub async fn run(&self) {
        while self.step().await {}
    }
}
