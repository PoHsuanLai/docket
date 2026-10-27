//! The running daemon: the files it reads, the seams it builds over the session bus, the router
//! over them, the bus served, the audit queue drained and the person's logout watched.

use crate::acp_gate::AcpGate;
use crate::audit::AuditLog;
use crate::builtin::{HostedLink, builtin_manifests};
use crate::builtin_checkpoints::CheckpointsPort;
use crate::builtin_companion::CompanionPort;
use crate::callers;
use crate::config::{ConfigError, IntentdConfig};
use crate::floor::{FLOOR_FILE, read_floor};
use crate::grants::FileGrants;
use crate::index_files::IndexStore;
use crate::infer::{InferdWriter, ReaderClient};
use crate::link::DbusLink;
use crate::logout::watch_logind;
use crate::manifests::{Loaded, intents_dir, load_manifests};
use crate::policies::{ADMIN_DIR, PolicyDirs, load_policies};
use crate::policies_watch::follow as follow_policies;
use crate::reach::{LiveReach, follow as follow_reach};
use crate::reviewers::reviewer;
use crate::search_files::ship_search_files;
use crate::serve::{ServeFault, closed, serve_on_gated};
use crate::settings_watch::{SettingsWatch, WatchState, apply, apply_next};
use crate::sheet::SheetConfirmer;
use crate::signals::{Cadence, pump};
use crate::space_watch::{RemovedMemories, SpaceKeeper};
use crate::system::{DaemonLog, SystemClock, SystemSeams};
use docket_checkpoint_git::{GitStore, StdGitRun};
use docket_core::{AbsPath, AuditRecord, Floor};
use docket_dbus::BusConnection;
use docket_dbus::InferWatch;
use docket_dbus::callers::{CallerTable, Identity};
use docket_memory::QueuedSink;
use docket_memory::{AlmanacMemory, AlmanacSessionLog};
use docket_router::{Router, Seams};
use docket_settings::{AgentSettings, Locator, REVIEW_CEILING};
use docket_skills::{Roots, discover};
use porter_daemon::ProcRoot;
use prov::SpaceId;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// How often the audit queue is drained into memoryd, unless the setup says otherwise.
const DRAIN_EVERY: Duration = Duration::from_secs(5);

/// How often the restore points past their keeping are forgotten (once at start, then daily).
const PRUNE_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// How long the daemon waits before following accountd's Spaces again after losing it.
const SPACES_RETRY: Duration = Duration::from_secs(10);

/// What only the acceptance harness's mains choose: where callers' cgroups are read from and
/// whether the inferd watch writes to a file. The shipped `main` passes the default (the system's
/// `/proc`, no watch) and no environment variable can change either.
#[derive(Debug, Clone)]
pub struct HarnessSeams {
    /// Where callers' cgroups are read.
    pub proc_root: ProcRoot,
    /// The system's callers file, when the harness's main chose another than `/etc/quire`'s: the
    /// harness is its own system, and a fixed role (the shell's) comes from no other file.
    pub system_callers: Option<PathBuf>,
    /// What is kept of the exchanges with inferd.
    pub trace: InferWatch,
    /// The administrator's floor file, when the harness's main chose another than `/etc/quire`'s.
    pub floor: Option<PathBuf>,
    /// The administrator's policy directory, when the harness's main chose another than
    /// `/etc/quire/permissions`.
    pub admin_permissions: Option<PathBuf>,
}

impl Default for HarnessSeams {
    fn default() -> Self {
        Self {
            proc_root: ProcRoot::System,
            system_callers: None,
            trace: InferWatch::off(),
            floor: None,
            admin_permissions: None,
        }
    }
}

/// Why the daemon did not start or stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DaemonFault {
    /// `intentd.toml` is not a configuration.
    #[error("{0}")]
    Config(ConfigError),
    /// The bus.
    #[error("{0}")]
    Serve(ServeFault),
    /// The policy set does not load: the shipped files, or an administrator's policy file, which
    /// is never skipped (a daemon that did would leave agents looser than the administrator meant).
    #[error("policy: {0}")]
    Policy(String),
    /// A directory the daemon keeps its files in is not an absolute path.
    #[error("not an absolute path: {0}")]
    State(String),
    /// `intentd-callers.toml` exists and cannot be used. A daemon that dropped it would identify
    /// fewer or more callers than it was told to.
    #[error("{0}")]
    Callers(String),
    /// The floor file exists and cannot be used. A daemon that skipped it would leave agents
    /// looser than the administrator meant.
    #[error("{0}")]
    Floor(String),
}

/// What a daemon starts from: the files it reads, found through the environment.
#[derive(Debug, Clone)]
pub struct Setup {
    /// The configuration.
    pub config: IntentdConfig,
    /// The installed manifests.
    pub manifests: Loaded,
    /// The consent file.
    pub grants: PathBuf,
    /// Where the private files of the restore points are kept (`quire/checkpoints` under the
    /// person's data directory, beside the consent file): never inside a repository.
    pub checkpoints: PathBuf,
    /// The person's login session, when known (`XDG_SESSION_ID`).
    pub session: Option<String>,
    /// Where what the apps pushed to the index is kept between logins.
    pub index: IndexStore,
    /// How often the audit queue is written to memoryd.
    pub audit_every: Duration,
    /// The data directories the manifests are read from, rescanned while the daemon runs.
    pub data_dirs: Vec<PathBuf>,
    /// How often the router's state is looked at for signals, and the manifests rescanned.
    pub signals: Cadence,
    /// Who is which unit: `intentd-callers.toml` of `/etc/quire` with the person's laid over it.
    pub callers: CallerTable,
    /// Says who a connection is, when a test names its own (`Identity::introduce_app`); otherwise
    /// porter's `ProcCallers` over `callers` and `proc_root`.
    pub identity: Option<Arc<Identity>>,
    /// Where callers' cgroups are read: `/proc`, unless the harness's main passed another root.
    pub proc_root: ProcRoot,
    /// Where the person's settings file is (`docket/settings.toml`, design/22 section 3.27).
    pub settings: Locator,
    /// What is kept of the exchanges with inferd (off, unless the harness's main passed a watch).
    pub trace: InferWatch,
    /// The administrator's floor: none unless the file exists (`run_with` reads it).
    pub floor: Floor,
    /// Where the policy files are read from, and watched.
    pub permissions: PolicyDirs,
}

fn home(env: &impl Fn(&str) -> Option<String>) -> PathBuf {
    env("HOME").map(PathBuf::from).unwrap_or_default()
}

fn listed(value: Option<String>, fallback: &str) -> Vec<PathBuf> {
    value
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
        .split(':')
        .map(PathBuf::from)
        .collect()
}

/// `$XDG_DATA_HOME` then `$XDG_DATA_DIRS`, with their defaults.
pub fn data_dirs(env: &impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let first = env("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home(env).join(".local/share"));
    let mut dirs = vec![first];
    dirs.extend(listed(env("XDG_DATA_DIRS"), "/usr/local/share:/usr/share"));
    dirs
}

/// `$XDG_CONFIG_HOME`, with its default.
fn config_home(env: &impl Fn(&str) -> Option<String>) -> PathBuf {
    env("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home(env).join(".config"))
}

/// `$XDG_CONFIG_HOME` then `$XDG_CONFIG_DIRS`, with their defaults.
fn config_dirs(env: &impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let mut dirs = vec![config_home(env)];
    dirs.extend(listed(env("XDG_CONFIG_DIRS"), "/etc/xdg"));
    dirs
}

impl Setup {
    /// Reads the configuration (the first `quire/intentd.toml` of the configuration
    /// directories, else the shipped one) and the manifests of the data directories.
    pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> Result<Self, DaemonFault> {
        let found = config_dirs(env)
            .into_iter()
            .map(|d| d.join("quire").join("intentd.toml"))
            .find_map(|path| std::fs::read_to_string(path).ok());
        let config = match found {
            Some(text) => IntentdConfig::parse(&text),
            None => IntentdConfig::shipped(),
        }
        .map_err(DaemonFault::Config)?;
        let callers = callers::read(env).map_err(|e| DaemonFault::Callers(e.to_string()))?;
        let data = data_dirs(env);
        let permissions = PolicyDirs {
            data_home: data[0].clone(),
            system_data: data[1..].to_vec(),
            person: config_home(env).join("quire").join("permissions"),
            admin: PathBuf::from(ADMIN_DIR),
        };
        let grants = intents_dir(&data[0]).join("grants.json");
        Ok(Self {
            config,
            manifests: load_manifests(&data),
            grants,
            checkpoints: data[0].join("quire").join("checkpoints"),
            session: env("XDG_SESSION_ID").filter(|v| !v.is_empty()),
            index: IndexStore::under(&data[0]),
            audit_every: DRAIN_EVERY,
            data_dirs: data,
            signals: Cadence::default(),
            callers,
            identity: None,
            proc_root: ProcRoot::System,
            settings: Locator::from_env(env),
            trace: InferWatch::off(),
            floor: Floor::none(),
            permissions,
        })
    }
}

/// A daemon that is serving.
#[derive(Debug)]
pub struct Running {
    tasks: Vec<tokio::task::JoinHandle<()>>,
    session: BusConnection,
}

impl Running {
    /// Waits until the session connection closes.
    pub async fn closed(&self) {
        closed(&self.session).await;
    }

    /// Stops serving: the background tasks end and the connection closes, so the name is free.
    pub async fn stop(self) {
        for task in &self.tasks {
            task.abort();
        }
        let _ = self.session.close().await;
    }
}

/// Builds the router over the seams of `session`, serves `org.quire.Intents1` on it, drains the
/// audit queue and, when `system` is given, ends the terminal's grants when logind removes the
/// person's session.
pub async fn start(
    session: &BusConnection,
    system: Option<&BusConnection>,
    setup: Setup,
) -> Result<Running, DaemonFault> {
    let Setup {
        config,
        manifests,
        grants,
        checkpoints,
        index,
        session: login,
        audit_every,
        data_dirs,
        signals,
        callers,
        identity,
        proc_root,
        settings,
        trace,
        floor,
        permissions,
    } = setup;
    for (file, why) in &manifests.skipped {
        eprintln!("intentd: skipped {}: {why}", file.display());
    }
    // The ACP name plays its roles only while the setting is on; the settings watch below keeps the
    // gate current, for the bus (who is an editor) and for the sheets (whose process is trusted).
    let acp = AcpGate::shut();
    let who = identity
        .unwrap_or_else(|| Arc::new(Identity::on(session.clone(), callers, proc_root.fixture())));
    let confirmer =
        SheetConfirmer::trusting(session.clone(), Arc::new(config.clone()), who.clone())
            .gated(acp.clone(), &proc_root);
    let port = CompanionPort::new();
    let restore_port = CheckpointsPort::new();
    let link = HostedLink::new(
        DbusLink::new(session.clone()),
        almanac_client::DbusTransport::new(session.clone()),
        port.clone(),
        restore_port.clone(),
    )
    .map_err(|e| DaemonFault::Policy(e.to_string()))?;
    let kept = AbsPath::parse(&checkpoints.to_string_lossy())
        .map_err(|_| DaemonFault::State(checkpoints.display().to_string()))?;
    let seams = SystemSeams {
        link,
        confirmer,
        // The router enforces the person's review times, live; the cascade's own are the ceiling.
        reviewer: reviewer(session, config.reviewers.as_ref(), REVIEW_CEILING, &trace)
            .ok_or_else(|| DaemonFault::Policy("no reviewer set".into()))?,
        grants: FileGrants::at(grants)
            .with_defaults(&data_dirs)
            .with_rules(permissions.clone()),
        sink: QueuedSink::new(),
        clock: SystemClock,
        memory: AlmanacMemory::over(almanac_client::DbusTransport::new(session.clone())),
        writer: InferdWriter::on_bus(session, trace.clone()),
        reader: ReaderClient::new(session.clone()),
        log: DaemonLog::Almanac(AlmanacSessionLog::over(
            almanac_client::DbusTransport::new(session.clone()),
            SpaceId::desktop(),
            SystemClock,
        )),
        checkpoints: GitStore::new(StdGitRun, kept),
    };
    // The policy files, layered: a bad person or assistant file is skipped and a bad preset falls
    // back to the built-in one (both logged); a bad administrator file stops the daemon.
    let loaded = load_policies(&permissions).map_err(|e| DaemonFault::Policy(e.to_string()))?;
    for rejected in &loaded.skipped {
        eprintln!("intentd: skipped policy {rejected}");
    }
    // The Settings page has no read-only row, so where the preset came from is said here.
    eprintln!("intentd: permissions: {}", loaded.preset);
    for dir in &loaded.unused {
        eprintln!("intentd: unused policy folder {}", dir.display());
    }
    let mut router = Router::new(seams, config.agent, loaded.presets);
    router.floor = floor;
    // The person's settings over intentd.toml's proposed values, now and whenever the file changes.
    let mut watched = SettingsWatch::start(settings, AgentSettings::over(config.agent));
    if let WatchState::Blind { reason } = watched.state() {
        eprintln!("intentd: settings are read once, not watched: {reason}");
    }
    apply(&router, &watched.current());
    {
        let mut state = router
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        for manifest in builtin_manifests()
            .map_err(|e| DaemonFault::Policy(e.to_string()))?
            .into_iter()
            .chain(manifests.manifests)
        {
            state.registry.insert(manifest);
        }
    }
    // What the apps ship and what they pushed in earlier logins is searchable from the start.
    ship_search_files(&router, &data_dirs);
    index.restore(&router);
    // The skills the data directories ship: valid files only; which are offered is checked against
    // the registry at each load. What did not load, or is hidden for a missing action, is logged.
    let found = discover(&Roots::from_dirs(&data_dirs));
    for rejected in &found.rejected {
        eprintln!(
            "intentd: skill {}: {}",
            rejected.dir.display(),
            rejected.fault
        );
    }
    router.install_skills(found.skills);
    router
        .hidden_skills()
        .iter()
        .for_each(|line| eprintln!("intentd: {line}"));
    // A session the log already holds keeps its name: nothing opened from now on takes it. Each
    // is restored when a request names it. Without memoryd the log is unreadable and the daemon
    // starts anyway; sessions then cannot be recorded or restored.
    if let Err(why) = router.adopt_sessions().await {
        eprintln!("intentd: the session log is not usable ({why}): sessions are not recorded");
        router.seams.log = DaemonLog::Off(docket_router::NoLog);
    }
    let router = Arc::new(router);
    port.attach(&router);
    restore_port.attach(&router);
    acp.set(watched.current().value.acp);
    acp.set_agents(watched.current().value.agents);
    serve_on_gated(
        session,
        router.clone(),
        Arc::new(config),
        who,
        &proc_root,
        acp.clone(),
        Some(index),
    )
    .await
    .map_err(DaemonFault::Serve)?;
    let mut tasks = Vec::new();
    let followed = router.clone();
    tasks.push(tokio::spawn(async move {
        while let Some(loaded) = apply_next(&followed, &mut watched).await {
            acp.set(loaded.value.acp);
            acp.set_agents(loaded.value.agents);
        }
    }));
    let live = LiveReach::none();
    tasks.push(tokio::spawn(follow_reach(
        router.clone(),
        session.clone(),
        data_dirs.clone(),
        live.clone(),
    )));
    tasks.push(tokio::spawn(pump(
        router.clone(),
        session.clone(),
        data_dirs,
        signals,
        live,
    )));
    tasks.push(tokio::spawn(
        SpaceKeeper::new(router.clone(), RemovedMemories::LeaveToMemory, SPACES_RETRY)
            .run(session.clone()),
    ));
    tasks.push(tokio::spawn(prune_checkpoints(router.clone())));
    tasks.push(tokio::spawn(follow_policies(router.clone(), permissions)));
    let queue = router.clone();
    let mut audit = AuditLog::over(almanac_client::DbusTransport::new(session.clone()));
    tasks.push(tokio::spawn(async move {
        loop {
            tokio::time::sleep(audit_every).await;
            let report = audit
                .flush(&queue.seams.sink, |record| placed_by(&queue, record))
                .await;
            count_lost(&queue, report.lost);
        }
    }));
    if let Some(system) = system {
        // Subscribed before `start` returns: a logout after that is never missed.
        match watch_logind(system).await {
            Ok(logouts) => {
                let router = router.clone();
                tasks.push(tokio::spawn(logouts.end_terminals(router, login)));
            }
            Err(why) => eprintln!("intentd: not watching logind: {why}"),
        }
    }
    Ok(Running {
        tasks,
        session: session.clone(),
    })
}

/// Forgets the restore points that are past their keeping, now and then every day: the newest
/// few of each session stay, and nothing older than the days the person keeps them.
async fn prune_checkpoints<S: Seams + 'static>(router: Arc<Router<S>>) {
    loop {
        let forgotten = router.prune_checkpoints().await;
        if forgotten.0 > 0 {
            eprintln!("intentd: forgot {} old restore points", forgotten.0);
        }
        tokio::time::sleep(PRUNE_EVERY).await;
    }
}

/// Adds records the log lost to the counter `Control.State` shows.
fn count_lost<S: Seams>(router: &Router<S>, lost: usize) {
    if lost == 0 {
        return;
    }
    let mut state = router
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let more = u32::try_from(lost).unwrap_or(u32::MAX);
    state.kill.audit_lost = porter_core::Count(state.kill.audit_lost.0.saturating_add(more));
}

/// Where a record that names no Space of its own happened, from the router's session and task
/// tables: a breaker trip is the session's, a task policy the task's, a halt the Space it covers.
fn placed_by<S: Seams>(router: &Router<S>, record: &AuditRecord) -> Option<SpaceId> {
    let state = router
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match record {
        AuditRecord::Breaker { session, .. } | AuditRecord::BriefConfirmed { session, .. } => {
            state.sessions.get(session).map(|r| r.space.clone())
        }
        AuditRecord::TaskPolicy { task, .. } | AuditRecord::PolicyCorrected { task, .. } => {
            state.tasks.get(task).map(|t| t.space.clone())
        }
        AuditRecord::Halt {
            scope: prov::SpaceScope::Only(space),
            ..
        } => Some(space.clone()),
        _ => None,
    }
}

/// The daemon: the session bus, the system bus for logind (absent, it is not watched), the
/// environment's files. Returns when the bus closes.
pub async fn run() -> Result<(), DaemonFault> {
    run_with(HarnessSeams::default()).await
}

/// [`run`] with the seams the acceptance harness's mains choose.
pub async fn run_with(seams: HarnessSeams) -> Result<(), DaemonFault> {
    let env = |key: &str| std::env::var(key).ok();
    let mut setup = Setup::from_env(&env)?;
    setup.proc_root = seams.proc_root;
    setup.trace = seams.trace;
    let floor = seams.floor.unwrap_or_else(|| PathBuf::from(FLOOR_FILE));
    setup.floor = read_floor(&floor).map_err(|e| DaemonFault::Floor(e.to_string()))?;
    setup.permissions.admin = seams
        .admin_permissions
        .unwrap_or_else(|| PathBuf::from(ADMIN_DIR));
    if let Some(system) = &seams.system_callers {
        setup.callers =
            callers::read_at(system, &env).map_err(|e| DaemonFault::Callers(e.to_string()))?;
    }
    let session = docket_dbus::session_connection(&env)
        .await
        .map_err(|e| DaemonFault::Serve(ServeFault::Bus(e.to_string())))?;
    let system = BusConnection::system().await.ok();
    if system.is_none() {
        eprintln!("intentd: no system bus: the terminal's grants end only when intentd does");
    }
    let running = start(&session, system.as_ref(), setup).await?;
    running.closed().await;
    Ok(())
}
