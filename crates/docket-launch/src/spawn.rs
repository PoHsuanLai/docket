//! `AgentSpawn`: the real `Spawn` behind `AcpBackend`. For one session it does a launcher's
//! duties in order, and undoes them in the reverse order whatever happens:
//!
//! 1. open a launcher session at accountd (`begin_session`);
//! 2. get the model by the entry's route: an inferd endpoint (P4, then a bridge to it if the
//!    sandbox's network is endpoint-only; or, for an allowlist network, the filter that stands
//!    between the agent and the sites it may reach), or a key handed to this one process (P2), or nothing
//!    (the agent signs itself in);
//! 3. build the environment from nothing (`env`), the binds from the entry, the network from the
//!    entry's mode, and start the process confined (`Procs`);
//! 4. remember a credential's process so a revocation can end it (`Registry`).
//!
//! `close` kills the process, revokes the credential, closes the endpoint, ends the session and
//! removes the socket directory. A process that is dropped without `close` is cleaned up the same
//! way on the runtime.

use crate::accounts::{Accounts, KeyHandoff, OpenedEndpoint, RouteWish};
use crate::config::{AgentsFile, EndpointKind, Entry, Profile, Route};
use crate::confine::Confinement;
use crate::egress;
use crate::env::{Lent, child_env};
use crate::names::launcher_session;
use crate::permit::AgentsPermit;
use crate::procs::{Proc, Procs};
use crate::protect;
use bulkhead::egress::{Proxy, Resolver, SystemResolver};
use bulkhead::forward::{Bridge, Loopback};
use bulkhead::{Access, AgentNet, AgentRun, Argv, Bind, EndpointBind, NetworkMode};
use docket_acp::bulk;
use docket_acp::client::{
    AgentChild, EdgeBind, LaunchPlan, SandboxStep, Spawn, SpawnFault, Spawned,
};
use docket_core::AbsPath;
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
    fn add(&self, id: ProcessCredentialId, proc: Arc<Mutex<P>>) {
        locked(&self.map).insert(id, proc);
    }

    fn remove(&self, id: &ProcessCredentialId) {
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
struct Release<A, P> {
    accounts: Arc<A>,
    registry: Registry<P>,
    session: LauncherSession,
    endpoint: Option<String>,
    credential: Option<ProcessCredentialId>,
    bridge: Option<Bridge>,
    proxy: Option<Proxy>,
    dir: Option<PathBuf>,
}

impl<A: Accounts, P: Proc> Release<A, P> {
    /// In the reverse of the order it was lent; each step is tried whatever the last did.
    async fn run(mut self) {
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
    proc: Arc<Mutex<P>>,
    release: Option<Release<A, P>>,
    net: NetworkMode,
    sites: Vec<String>,
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

/// Starts the programs listed in `agents.toml`.
pub struct AgentSpawn<A: Accounts + 'static, P: Procs> {
    file: Arc<AgentsFile>,
    accounts: Arc<A>,
    procs: P,
    run_dir: PathBuf,
    forwarder: AbsPath,
    registry: Registry<P::Proc>,
    protected: Vec<AbsPath>,
    enter: Option<AbsPath>,
    resolver: Arc<dyn Resolver>,
}

impl<A: Accounts + 'static, P: Procs> std::fmt::Debug for AgentSpawn<A, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentSpawn").finish_non_exhaustive()
    }
}

impl<A: Accounts + 'static, P: Procs> AgentSpawn<A, P> {
    /// A spawner for the programs in `file`, allowed by `permit`. Per-session socket directories
    /// go under `run_dir` (a directory only the user can enter, such as `$XDG_RUNTIME_DIR`);
    /// `forwarder` is the `docket-net-forward` program; `registry` is shared with the
    /// supervisor.
    pub fn new(
        _permit: AgentsPermit,
        file: Arc<AgentsFile>,
        accounts: Arc<A>,
        procs: P,
        run_dir: PathBuf,
        forwarder: AbsPath,
        registry: Registry<P::Proc>,
    ) -> Self {
        Self {
            file,
            accounts,
            procs,
            run_dir,
            forwarder,
            registry,
            protected: Vec::new(),
            enter: None,
            resolver: Arc::new(SystemResolver),
        }
    }

    /// Starts every agent through the exec stage at `enter` (`docket-net-enter`), which limits
    /// what the agent may do with the files and the network it is given. Without it the agent
    /// is held by the sandbox alone, and its confinement says so.
    #[must_use]
    pub fn with_landlock(mut self, enter: AbsPath) -> Self {
        self.enter = Some(enter);
        self
    }

    /// Resolves the names of an `allowlist` agent's requests with `resolver` instead of the
    /// system's: for a test, which has no network.
    #[must_use]
    pub fn with_resolver(mut self, resolver: Arc<dyn Resolver>) -> Self {
        self.resolver = resolver;
        self
    }

    /// Paths kept read-only wherever they fall inside an agent's writable workspace, on top of
    /// every `.git` directory (always protected): the checkpoint repository, say. A path
    /// outside the workspace is not the agent's to write anyway and is ignored.
    #[must_use]
    pub fn with_protected(mut self, paths: Vec<AbsPath>) -> Self {
        self.protected = paths;
        self
    }
}

pub(crate) fn binds(
    entry: &Entry,
    key_file: Option<&AbsPath>,
    edge: Option<&EdgeBind>,
) -> Result<Vec<Bind>, bulkhead::PathFault> {
    let program_dir = entry.command.parent().into_iter();
    let read_only = entry
        .reads
        .iter()
        .cloned()
        .chain(program_dir)
        .chain(key_file.cloned())
        .chain(edge.map(|e| e.bridge.clone()));
    // The tool edge's socket is written to by whoever connects, so it is the one read-write bind
    // that is not the program's own state.
    let socket = edge.map(|e| (e.socket.clone(), Access::ReadWrite));
    read_only
        .map(|path| (path, Access::ReadOnly))
        .chain(socket)
        .chain(
            entry
                .state
                .iter()
                .cloned()
                .map(|path| (path, Access::ReadWrite)),
        )
        .map(|(path, access)| Ok(Bind::new(bulk::to_shell(&path)?, access)))
        .collect()
}

/// The run's private directory (mode 0700), made on first use and removed with the session.
fn session_dir<A: Accounts, P: Proc>(
    run_dir: &std::path::Path,
    session: &LauncherSession,
    release: &mut Release<A, P>,
) -> std::io::Result<PathBuf> {
    use std::os::unix::fs::DirBuilderExt;
    if let Some(dir) = &release.dir {
        return Ok(dir.clone());
    }
    let dir = run_dir.join(format!("docket-agent-{}", session.as_str()));
    std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
    release.dir = Some(dir.clone());
    Ok(dir)
}

impl<A: Accounts + 'static, P: Procs> Spawn for AgentSpawn<A, P> {
    type Wire = P::Wire;
    type Child = Child<A, P::Proc>;

    async fn spawn(
        &mut self,
        plan: &LaunchPlan,
    ) -> Result<Spawned<P::Wire, Child<A, P::Proc>>, SpawnFault> {
        let file = Arc::clone(&self.file);
        let entry = file.get(&plan.program).ok_or(SpawnFault::NotAllowed)?;
        let session = launcher_session(&plan.session).ok_or(SpawnFault::Accounts)?;
        self.accounts
            .begin_session(&session)
            .await
            .map_err(|_| SpawnFault::Accounts)?;
        let mut release = Release {
            accounts: Arc::clone(&self.accounts),
            registry: self.registry.clone(),
            session: session.clone(),
            endpoint: None,
            credential: None,
            bridge: None,
            proxy: None,
            dir: None,
        };
        match self.launch(entry, plan, &session, &mut release).await {
            Ok((wire, proc)) => Ok(Spawned {
                wire,
                child: Child {
                    proc,
                    release: Some(release),
                    net: entry.network,
                    sites: entry
                        .reach
                        .as_ref()
                        .map(|r| r.sites().to_vec())
                        .unwrap_or_default(),
                },
                meta: entry.profile.and_then(Profile::session_meta),
                sign_in: entry.sign_in.clone(),
                model: entry.model.clone(),
            }),
            Err(fault) => {
                release.run().await;
                Err(fault)
            }
        }
    }
}

/// The loopback port inside the sandbox where an allowlist agent finds its filter (the usual
/// proxy port; the agent's network is its own, so nothing else of ours is there).
const PROXY_PORT: u16 = 3128;

/// What a route lent, kept alive until the process is built.
enum Got {
    Nothing,
    Endpoint(OpenedEndpoint),
    Key(KeyHandoff),
}

impl<A: Accounts + 'static, P: Procs> AgentSpawn<A, P> {
    async fn lend(
        accounts: &A,
        entry: &Entry,
        session: &LauncherSession,
        release: &mut Release<A, P::Proc>,
    ) -> Result<Got, SpawnFault> {
        match entry.route {
            Route::Login => Ok(Got::Nothing),
            Route::Endpoint => {
                let target = entry.endpoint.as_ref().ok_or(SpawnFault::NotAllowed)?;
                let wish = RouteWish {
                    kind: match target.kind {
                        EndpointKind::Account => "account",
                        EndpointKind::Model => "model",
                    },
                    id: target.id.clone(),
                    models: target.models.clone(),
                };
                let opened = accounts
                    .open_endpoint(&entry.program, &wish, entry.class, entry.protocol)
                    .await
                    .map_err(|_| SpawnFault::Accounts)?;
                release.endpoint = Some(opened.session.clone());
                Ok(Got::Endpoint(opened))
            }
            Route::Handoff => {
                let key_env = entry.key_env.as_ref().ok_or(SpawnFault::NotAllowed)?;
                let permit = accounts
                    .request_key(&entry.program, entry.class, session)
                    .await
                    .map_err(|_| SpawnFault::Accounts)?;
                let issued = accounts
                    .issue(&permit, &entry.program, key_env, entry.delivery)
                    .await
                    .map_err(|_| SpawnFault::Accounts)?;
                release.credential = Some(issued.id);
                Ok(Got::Key(issued.key))
            }
        }
    }

    /// The network plan: for endpoint-only a socket directory, a bridge to the endpoint, and the
    /// forwarder's binds; for an allowlist the same socket directory with the filter on it; for
    /// host and none nothing more. A mode this build does not know starts nothing.
    fn network(
        &self,
        entry: &Entry,
        got: &Got,
        session: &LauncherSession,
        release: &mut Release<A, P::Proc>,
    ) -> Result<AgentNet, SpawnFault> {
        match entry.network {
            NetworkMode::EndpointOnly => self.endpoint_net(got, session, release),
            NetworkMode::Allowlist => self.allowlist_net(entry, session, release),
            NetworkMode::None | NetworkMode::Host => {
                AgentNet::of(entry.network, None).map_err(|_| SpawnFault::Sandbox(SandboxStep::NetworkPlan))
            }
            _ => Err(SpawnFault::Sandbox(SandboxStep::NetworkMode)),
        }
    }

    /// The session's socket (not yet listening) and the bind that has the forwarder reach it
    /// from inside, on loopback `port`.
    fn bind(
        &self,
        session: &LauncherSession,
        release: &mut Release<A, P::Proc>,
        port: u16,
    ) -> Result<(PathBuf, EndpointBind), SpawnFault> {
        let dir = session_dir(&self.run_dir, session, release)
            .map_err(|_| SpawnFault::Sandbox(SandboxStep::SessionDir))?;
        let socket = dir.join("ep.sock");
        let inside = socket
            .to_str()
            .and_then(|t| bulkhead::AbsPath::parse(t).ok())
            .ok_or(SpawnFault::Sandbox(SandboxStep::SocketPath))?;
        let forwarder = bulk::to_shell(&self.forwarder).map_err(|_| SpawnFault::Sandbox(SandboxStep::Forwarder))?;
        Ok((socket, EndpointBind::new(forwarder, inside, port)))
    }

    fn endpoint_net(
        &self,
        got: &Got,
        session: &LauncherSession,
        release: &mut Release<A, P::Proc>,
    ) -> Result<AgentNet, SpawnFault> {
        let Got::Endpoint(endpoint) = got else {
            return Err(SpawnFault::Sandbox(SandboxStep::NoEndpoint));
        };
        let target =
            Loopback::new(&endpoint.host, endpoint.port).map_err(|_| SpawnFault::Accounts)?;
        let (socket, bind) = self.bind(session, release, target.port())?;
        release.bridge = Some(Bridge::start(&socket, target).map_err(|_| SpawnFault::Sandbox(SandboxStep::Bridge))?);
        AgentNet::of(NetworkMode::EndpointOnly, Some(bind))
            .map_err(|_| SpawnFault::Sandbox(SandboxStep::NetworkPlan))
    }

    /// The filter listens on the socket for exactly the life of the session (`Release` holds
    /// it); the agent finds it on loopback `PROXY_PORT` inside its own network.
    fn allowlist_net(
        &self,
        entry: &Entry,
        session: &LauncherSession,
        release: &mut Release<A, P::Proc>,
    ) -> Result<AgentNet, SpawnFault> {
        let reach = entry.reach.as_ref().ok_or(SpawnFault::Sandbox(SandboxStep::NoReach))?;
        let (socket, bind) = self.bind(session, release, PROXY_PORT)?;
        release.proxy = Some(
            egress::start(&socket, reach, Arc::clone(&self.resolver))
                .map_err(|_| SpawnFault::Sandbox(SandboxStep::Filter))?,
        );
        AgentNet::of(NetworkMode::Allowlist, Some(bind))
            .map_err(|_| SpawnFault::Sandbox(SandboxStep::NetworkPlan))
    }

    /// `run` with every `.git` directory of the workspace, and the named extra paths that lie
    /// in it, bound read-only over the writable workspace. A `.git` that cannot be protected
    /// stops the start; an extra outside the workspace is not the agent's and is left out.
    fn guarded(&self, run: AgentRun, cwd: &bulkhead::AbsPath) -> Result<AgentRun, SpawnFault> {
        let run = protect::git_dirs(cwd)
            .into_iter()
            .try_fold(run, AgentRun::with_protect)
            .map_err(|_| SpawnFault::Sandbox(SandboxStep::Protect))?;
        Ok(self
            .protected
            .iter()
            .filter_map(|path| bulk::to_shell(path).ok())
            .fold(run, |run, path| {
                run.clone().with_protect(path).unwrap_or(run)
            }))
    }

    async fn launch(
        &mut self,
        entry: &Entry,
        plan: &LaunchPlan,
        session: &LauncherSession,
        release: &mut Release<A, P::Proc>,
    ) -> Result<(P::Wire, Arc<Mutex<P::Proc>>), SpawnFault> {
        let got = Self::lend(&self.accounts, entry, session, release).await?;
        let net = self.network(entry, &got, session, release)?;
        let lent = match &got {
            Got::Nothing => Lent::Nothing,
            Got::Endpoint(e) => Lent::Endpoint(e),
            Got::Key(k) => Lent::Key(k),
        };
        let built = child_env(entry, &lent).map_err(|_| SpawnFault::Sandbox(SandboxStep::Environment))?;
        let command = entry.command.as_str();
        let cwd = bulk::to_shell(&plan.cwd).map_err(|_| SpawnFault::Sandbox(SandboxStep::WorkingFolder))?;
        let run = binds(entry, built.key_file.as_ref(), plan.edge.as_ref())
            .map_err(|_| SpawnFault::Sandbox(SandboxStep::Binds))?
            .into_iter()
            .fold(
                AgentRun::new(
                    Argv::new(command, &entry.args).ok_or(SpawnFault::Process)?,
                    cwd.clone(),
                    built.vars,
                    net,
                ),
                AgentRun::with_bind,
            );
        let run = self.guarded(run, &cwd)?;
        let run = match &self.enter {
            Some(enter) => {
                run.with_landlock(
                    bulk::to_shell(enter).map_err(|_| SpawnFault::Sandbox(SandboxStep::Enter))?,
                )
            }
            None => run,
        };
        let (wire, proc) = self
            .procs
            .start(&run)
            .await
            .map_err(|_| SpawnFault::Sandbox(SandboxStep::Start))?;
        let proc = Arc::new(Mutex::new(proc));
        if let Some(id) = &release.credential {
            self.registry.add(id.clone(), Arc::clone(&proc));
        }
        Ok((wire, proc))
    }
}
