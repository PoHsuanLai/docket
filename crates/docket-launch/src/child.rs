//! What one running agent holds and gives back: the `Registry` of processes by credential, the
//! `Release` of what was lent for a session, and the `Child` the backend keeps.

use crate::accounts::Accounts;
use crate::confine::Confinement;
use crate::procs::Proc;
use bulkhead::NetworkMode;
use bulkhead::egress::Proxy;
use bulkhead::forward::Bridge;
use docket_acp::client::AgentChild;
use porter_core::{LauncherSession, ProcessCredentialId};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// The live processes that hold a credential, by credential: what a revocation looks up.
#[derive(Debug)]
pub struct Registry<P> {
    map: Arc<Mutex<BTreeMap<ProcessCredentialId, Arc<Mutex<P>>>>>,
}

impl<P> Clone for Registry<P> {
    fn clone(&self) -> Self {
        Self {
            map: Arc::clone(&self.map),
        }
    }
}

impl<P: Proc> Default for Registry<P> {
    fn default() -> Self {
        Self {
            map: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

impl<P: Proc> Registry<P> {
    pub(crate) fn add(&self, id: ProcessCredentialId, proc: Arc<Mutex<P>>) {
        locked(&self.map).insert(id, proc);
    }

    pub(crate) fn remove(&self, id: &ProcessCredentialId) {
        locked(&self.map).remove(id);
    }

    /// Ends the process that holds `id`. True if there was one.
    pub fn kill(&self, id: &ProcessCredentialId) -> bool {
        let proc = locked(&self.map).get(id).cloned();
        proc.map(|p| locked(&p).kill()).is_some()
    }

    /// How many processes hold a credential.
    pub fn len(&self) -> usize {
        locked(&self.map).len()
    }

    /// Whether none do.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

/// What was lent for one session, to give back.
pub(crate) struct Release<A, P> {
    pub(crate) accounts: Arc<A>,
    pub(crate) registry: Registry<P>,
    pub(crate) session: LauncherSession,
    pub(crate) endpoint: Option<String>,
    pub(crate) credential: Option<ProcessCredentialId>,
    pub(crate) bridge: Option<Bridge>,
    pub(crate) proxy: Option<Proxy>,
    pub(crate) dir: Option<PathBuf>,
}

impl<A: Accounts, P: Proc> Release<A, P> {
    /// In the reverse of the order it was lent; each step is tried whatever the last did.
    pub(crate) async fn run(mut self) {
        if let Some(id) = self.credential.take() {
            self.registry.remove(&id);
            let _ = self.accounts.revoke(&id).await;
        }
        if let Some(endpoint) = self.endpoint.take() {
            let _ = self.accounts.close_endpoint(&endpoint).await;
        }
        let _ = self.accounts.end_session(&self.session).await;
        drop(self.bridge.take());
        drop(self.proxy.take());
        if let Some(dir) = self.dir.take() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// The running agent as the backend holds it.
pub struct Child<A: Accounts + 'static, P: Proc + 'static> {
    pub(crate) proc: Arc<Mutex<P>>,
    pub(crate) release: Option<Release<A, P>>,
    pub(crate) net: NetworkMode,
    pub(crate) sites: Vec<String>,
}

impl<A: Accounts + 'static, P: Proc + 'static> Child<A, P> {
    /// How far this agent is held, as a person reads it: what the kernel limits (known once the
    /// exec stage has reported, so ask after the session opens) and which network it has.
    pub fn confinement(&self) -> Confinement {
        Confinement::new(locked(&self.proc).landlock(), self.net, self.sites.clone())
    }
}

impl<A: Accounts + 'static, P: Proc + 'static> std::fmt::Debug for Child<A, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Child").finish_non_exhaustive()
    }
}

impl<A: Accounts + 'static, P: Proc + 'static> AgentChild for Child<A, P> {
    fn kill(&mut self) {
        locked(&self.proc).kill();
    }

    async fn close(&mut self) {
        self.kill();
        if let Some(release) = self.release.take() {
            release.run().await;
        }
    }
}

impl<A: Accounts + 'static, P: Proc + 'static> Drop for Child<A, P> {
    fn drop(&mut self) {
        self.kill();
        if let (Some(release), Ok(runtime)) =
            (self.release.take(), tokio::runtime::Handle::try_current())
        {
            runtime.spawn(release.run());
        }
    }
}
