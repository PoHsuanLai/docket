//! The running daemon: the files it reads, the seams it builds over the session bus, the router
//! over them, the bus served, the audit queue drained and the person's logout watched.

use crate::audit::AuditLog;
use crate::builtin::{HostedLink, builtin_manifests};
use crate::builtin_companion::CompanionPort;
use crate::config::{ConfigError, IntentdConfig};
use crate::grants::FileGrants;
use crate::infer::{InferdWriter, ReaderClient};
use crate::link::DbusLink;
use crate::logout::watch_logind;
use crate::manifests::{Loaded, intents_dir, load_manifests};
use crate::memory::AlmanacMemory;
use crate::reviewers::reviewer;
use crate::serve::{ServeFault, closed, serve_on};
use crate::sheet::SheetConfirmer;
use crate::sink::QueuedSink;
use crate::system::{SystemClock, SystemSeams};
use docket_core::AuditRecord;
use docket_dbus::BusConnection;
use docket_router::{Router, Seams};
use policy_point::Pdp;
use prov::SpaceId;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// How often the audit queue is drained into memoryd, unless the setup says otherwise.
const DRAIN_EVERY: Duration = Duration::from_secs(5);

/// Why the daemon did not start or stopped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DaemonFault {
    /// `intentd.toml` is not a configuration.
    #[error("{0}")]
    Config(ConfigError),
    /// The bus.
    #[error("{0}")]
    Serve(ServeFault),
    /// The shipped policy set does not load.
    #[error("policy: {0}")]
    Policy(String),
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
    /// The person's login session, when known (`XDG_SESSION_ID`).
    pub session: Option<String>,
    /// How often the audit queue is written to memoryd.
    pub audit_every: Duration,
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

/// `$XDG_CONFIG_HOME` then `$XDG_CONFIG_DIRS`, with their defaults.
fn config_dirs(env: &impl Fn(&str) -> Option<String>) -> Vec<PathBuf> {
    let first = env("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home(env).join(".config"));
    let mut dirs = vec![first];
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
        let data = data_dirs(env);
        let grants = intents_dir(&data[0]).join("grants.json");
        Ok(Self {
            config,
            manifests: load_manifests(&data),
            grants,
            session: env("XDG_SESSION_ID").filter(|v| !v.is_empty()),
            audit_every: DRAIN_EVERY,
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
        session: login,
        audit_every,
    } = setup;
    for (file, why) in &manifests.skipped {
        eprintln!("intentd: skipped {}: {why}", file.display());
    }
    let confirmer = SheetConfirmer::trusting(session.clone(), Arc::new(config.clone()));
    let port = CompanionPort::new();
    let link = HostedLink::new(
        DbusLink::new(session.clone()),
        almanac_client::DbusTransport::new(session.clone()),
        port.clone(),
    )
    .map_err(|e| DaemonFault::Policy(e.to_string()))?;
    let seams = SystemSeams {
        link,
        confirmer,
        reviewer: reviewer(session, config.reviewers.as_ref(), config.agent.review)
            .ok_or_else(|| DaemonFault::Policy("no reviewer set".into()))?,
        grants: FileGrants::at(grants),
        sink: QueuedSink::new(),
        clock: SystemClock,
        memory: AlmanacMemory::over(almanac_client::DbusTransport::new(session.clone())),
        writer: InferdWriter::on_bus(session),
        reader: ReaderClient::new(session.clone()),
    };
    let pdp = Pdp::standard().map_err(|e| DaemonFault::Policy(e.to_string()))?;
    let router = Router::new(seams, config.agent, pdp);
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
    let router = Arc::new(router);
    port.attach(&router);
    serve_on(session, router.clone(), Arc::new(config))
        .await
        .map_err(DaemonFault::Serve)?;
    let mut tasks = Vec::new();
    let queue = router.clone();
    let mut audit = AuditLog::over(almanac_client::DbusTransport::new(session.clone()));
    tasks.push(tokio::spawn(async move {
        loop {
            tokio::time::sleep(audit_every).await;
            audit
                .flush(&queue.seams.sink, |record| placed_by(&queue, record))
                .await;
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

/// Where a record that names no Space of its own happened, from the router's session and task
/// tables: a breaker trip is the session's, a task policy the task's, a halt the Space it covers.
fn placed_by<S: Seams>(router: &Router<S>, record: &AuditRecord) -> Option<SpaceId> {
    let state = router
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match record {
        AuditRecord::Breaker { session, .. } => {
            state.sessions.get(session).map(|r| r.space.clone())
        }
        AuditRecord::TaskPolicy { task, .. } => state.tasks.get(task).map(|t| t.space.clone()),
        AuditRecord::Halt {
            scope: prov::SpaceScope::Only(space),
            ..
        } => Some(space.clone()),
        _ => None,
    }
}

/// The session bus: `$DBUS_SESSION_BUS_ADDRESS`, else the address of the bus that started this
/// process by activation (`$DBUS_STARTER_ADDRESS`), else the default per-user socket.
async fn session_bus(env: &impl Fn(&str) -> Option<String>) -> zbus::Result<BusConnection> {
    let address = env("DBUS_SESSION_BUS_ADDRESS")
        .or_else(|| env("DBUS_STARTER_ADDRESS"))
        .filter(|a| !a.is_empty());
    match address {
        Some(address) => {
            zbus::connection::Builder::address(address.as_str())?
                .build()
                .await
        }
        None => BusConnection::session().await,
    }
}

/// The daemon: the session bus, the system bus for logind (absent, it is not watched), the
/// environment's files. Returns when the bus closes.
pub async fn run() -> Result<(), DaemonFault> {
    let env = |key: &str| std::env::var(key).ok();
    let setup = Setup::from_env(&env)?;
    let session = session_bus(&env)
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
