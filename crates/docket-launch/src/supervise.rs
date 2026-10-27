//! The launcher's long-running duties: register the programs, then listen. A revocation ends the
//! process that holds the credential (accountd cannot reach the child's copy of a key). A login
//! or logout request runs the agent's own command visibly and reports only the coarse outcome.
//! A login waits on a person, so each runs in a task of its own under a deadline: a revocation
//! is handled the moment it is heard, never behind one.

use crate::accounts::{Accounts, Heard, LoginAsk};
use crate::child::Registry;
use crate::config::AgentsFile;
use crate::login::LoginRunner;
use crate::procs::Proc;
use docket_store::{Waits, within};
use porter_core::{LoginFault, LoginOutcome};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;
use tokio::task::JoinHandle;

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Listens for what porter says to this launcher.
pub struct Supervisor<A, P, L> {
    accounts: Arc<A>,
    file: Arc<AgentsFile>,
    registry: Registry<P>,
    login: Arc<L>,
    /// How long one login or logout may take (a person is in it).
    span: Duration,
    /// Held while a login runs: two terminals at once would only confuse the person.
    one_at_a_time: Arc<tokio::sync::Mutex<()>>,
    running: Mutex<Vec<JoinHandle<()>>>,
}

impl<A, P, L> std::fmt::Debug for Supervisor<A, P, L> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Supervisor").finish_non_exhaustive()
    }
}

/// One login or logout, in a task of its own.
struct Job<A, L> {
    accounts: Arc<A>,
    file: Arc<AgentsFile>,
    login: Arc<L>,
    span: Duration,
    one_at_a_time: Arc<tokio::sync::Mutex<()>>,
}

impl<A: Accounts, L: LoginRunner> Job<A, L> {
    /// Waits its turn, runs the command under the deadline and reports the outcome.
    async fn run(self, ask: LoginAsk) {
        let _turn = self.one_at_a_time.lock().await;
        // A program we did not list gets no command: the outcome says it is not here.
        let outcome = match self.file.by_program(&ask.program) {
            Some(entry) => {
                // Boxed: a deeper future here once exhausted the stack.
                let work = Box::pin(self.login.run(entry, ask.kind));
                within(tokio::time::sleep(self.span), work)
                    .await
                    // The command is dropped (and with it killed) when the span runs out.
                    .unwrap_or(LoginOutcome::Failed(LoginFault::TimedOut))
            }
            None => LoginOutcome::Failed(LoginFault::NotInstalled),
        };
        let _ = self.accounts.report(&ask, outcome).await;
    }
}

impl<A: Accounts + 'static, P: Proc, L: LoginRunner + 'static> Supervisor<A, P, L> {
    /// A supervisor over the shared link and registry.
    pub fn new(accounts: Arc<A>, file: Arc<AgentsFile>, registry: Registry<P>, login: L) -> Self {
        Self {
            accounts,
            file,
            registry,
            login: Arc::new(login),
            span: Waits::DEFAULT.person,
            one_at_a_time: Arc::new(tokio::sync::Mutex::new(())),
            running: Mutex::new(Vec::new()),
        }
    }

    /// The same supervisor with `span` as the longest one login or logout may take.
    #[must_use]
    pub fn with_login_span(mut self, span: Duration) -> Self {
        self.span = span;
        self
    }

    /// Registers the programs. A program another launcher holds is a fault, and none is taken.
    pub async fn register(&self) -> Result<(), crate::accounts::AccountFault> {
        self.accounts.register(&self.file.programs()).await
    }

    /// Handles one thing porter said. False when the link is gone. A revocation is done when
    /// this returns; a login or logout is started in its own task, so this does not wait on it.
    pub async fn step(&self) -> bool {
        let Some(heard) = self.accounts.hear().await else {
            return false;
        };
        match heard {
            Heard::Revoked(id) => {
                self.registry.kill(&id);
            }
            Heard::Ask(ask) => self.start(ask),
        }
        true
    }

    fn start(&self, ask: LoginAsk) {
        let job = Job {
            accounts: Arc::clone(&self.accounts),
            file: Arc::clone(&self.file),
            login: Arc::clone(&self.login),
            span: self.span,
            one_at_a_time: Arc::clone(&self.one_at_a_time),
        };
        let task = tokio::spawn(job.run(ask));
        let mut running = locked(&self.running);
        running.retain(|t| !t.is_finished());
        running.push(task);
    }

    /// Waits for the logins and logouts still running (each ends by its deadline).
    pub async fn finish(&self) {
        let tasks = std::mem::take(&mut *locked(&self.running));
        for task in tasks {
            let _ = task.await;
        }
    }

    /// Listens until the link ends, then lets the running logins finish.
    pub async fn run(&self) {
        while self.step().await {}
        self.finish().await;
    }
}
