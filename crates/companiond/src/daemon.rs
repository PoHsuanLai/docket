//! The daemon: configuration, the session bus, the router and inferd over it, a restart that
//! rebuilds the roster and the front task from the eventlog, and `Companion1` served.

use crate::Companiond;
use crate::clock::Clock;
use crate::config::CompaniondConfig;
use crate::handoff::HostLink;
use crate::helpers::Hub;
use crate::serve::serve_on_rooted;
use docket_client::{DbusTransport, Intents};
use docket_core::helper::HelperCard;
use docket_dbus::callers::{CallerTable, Identity};
use docket_dbus::tap::Tap;
use docket_dbus::{BusConnection, InferLink};
use docket_planner::PlannerModel;
use docket_skills::{Roots, discover};
use docket_tasks::ServeFault;
use futures_util::StreamExt;
use futures_util::lock::Mutex;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What only the acceptance harness's mains choose: the `/proc` callers are read from (a tree
/// other than `/proc` is a fixture, whose units' main processes are in `<root>/units`), the
/// system's callers file, and whether the model tap writes to a file. The shipped `main` passes
/// the default (the system's `/proc`, `/etc/quire`, no tap) and no environment variable can
/// change any.
#[derive(Debug, Clone)]
pub struct HarnessSeams {
    /// Where callers' cgroups are read.
    pub proc_root: PathBuf,
    /// The system's callers file, when the harness's main chose another than `/etc/quire`'s: the
    /// harness is its own system, and a fixed role (the shell's) comes from no other file.
    pub system_callers: Option<PathBuf>,
    /// What is kept of the exchanges with inferd.
    pub trace: Tap,
}

impl Default for HarnessSeams {
    fn default() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
            system_callers: None,
            trace: Tap::off(),
        }
    }
}

/// The companion as the daemon runs it: inferd and the router over the session bus.
pub type Daemon = Companiond<InferLink, DbusTransport>;

/// Builds the companion over `connection`, takes up what a restart finds in the eventlog (a
/// router or memoryd that is not there yet leaves it with nothing to resume, never a failure) and
/// serves `org.quire.Companion1`. Returns once the name is ours; the connection keeps serving
/// until it closes.
pub async fn start(
    connection: &BusConnection,
    config: CompaniondConfig,
    callers: CallerTable,
) -> Result<Arc<Mutex<Daemon>>, ServeFault> {
    start_with(
        connection,
        config,
        &Roots::default(),
        HarnessSeams::default(),
        callers,
    )
    .await
}

/// [`start`] with the skill directories the daemon's `main` found: the only place skill text
/// can come from. A directory that does not load is named on stderr and left out.
pub async fn start_with(
    connection: &BusConnection,
    config: CompaniondConfig,
    skills: &Roots,
    seams: HarnessSeams,
    callers: CallerTable,
) -> Result<Arc<Mutex<Daemon>>, ServeFault> {
    let found = discover(skills);
    for rejected in &found.rejected {
        tracing::warn!(
            dir = %rejected.dir.display(),
            fault = %rejected.fault,
            "a skill directory did not load; left out"
        );
    }
    let intents = Intents::over(DbusTransport::new(connection.clone()));
    let planner = PlannerModel::new(docket_dbus::inferd_transport(
        connection,
        seams.trace.clone(),
    ));
    let mut companion = Companiond::new(
        intents,
        planner,
        config.agent.clone(),
        Clock::System,
        config.shell.clone(),
    )
    .with_skills(found.skills)
    // The coding agent the person may choose as the assistant's model takes the turn through its
    // host, which the bus starts on the first call.
    .with_handoff(Arc::new(HostLink::new(connection.clone())));
    // The helpers the person keeps: the board is owned here, and each job runs on a companion of
    // its own, built over the same bus and asking inferd as its helper.
    let hub = {
        let (link, trace) = (connection.clone(), seams.trace.clone());
        let (agent, shell) = (config.agent.clone(), config.shell.clone());
        let make = move |card: &HelperCard| {
            Companiond::new(
                Intents::over(DbusTransport::new(link.clone())),
                docket_tasks::planner_for(
                    docket_dbus::inferd_transport(&link, trace.clone()),
                    card,
                ),
                agent.clone(),
                Clock::System,
                shell.clone(),
            )
        };
        // Reads and writes the helpers' records; it never asks a model.
        let service = Companiond::new(
            Intents::over(DbusTransport::new(connection.clone())),
            PlannerModel::new(docket_dbus::inferd_transport(connection, Tap::off())),
            config.agent.clone(),
            Clock::System,
            config.shell.clone(),
        );
        Hub::new(service, companion.shared.clone(), make)
    };
    companion.set_desk(Arc::new(hub.clone()));
    // A router or memoryd that hangs must not keep the name from being claimed.
    match tokio::time::timeout(companion.waits.startup, companion.restore(&config.spaces)).await {
        Ok(Ok(restarted)) => tracing::info!(
            restored = restarted.restored,
            failed = restarted.failed,
            resumed = restarted.opened.is_some(),
            "restart read"
        ),
        Ok(Err(cause)) => tracing::warn!(%cause, "restart found nothing to resume"),
        Err(_) => tracing::warn!("the restart did not finish in time; starting without it"),
    }
    if tokio::time::timeout(companion.waits.startup, hub.restore(&config.spaces))
        .await
        .is_err()
    {
        tracing::warn!("the helpers did not come back in time; starting without them");
    }
    let companion = Arc::new(Mutex::new(companion));
    let fixture = (seams.proc_root != Path::new("/proc")).then_some(seams.proc_root.as_path());
    let who = Arc::new(Identity::on(connection.clone(), callers, fixture));
    serve_on_rooted(connection, companion.clone(), who, seams.proc_root).await?;
    Ok(companion)
}

/// The daemon: its configuration and the session bus from the environment, served until the bus
/// closes.
pub async fn run() -> Result<(), ServeFault> {
    run_with(HarnessSeams::default()).await
}

/// [`run`] with the seams the acceptance harness's mains choose.
pub async fn run_with(seams: HarnessSeams) -> Result<(), ServeFault> {
    let env = |key: &str| std::env::var(key).ok();
    let config = CompaniondConfig::from_env(&env).map_err(|e| ServeFault::Config(e.to_string()))?;
    let connection = docket_dbus::session_connection(&env)
        .await
        .map_err(|e| ServeFault::Bus(e.to_string()))?;
    let callers = match &seams.system_callers {
        Some(system) => crate::callers::read_at(system, &env),
        None => crate::callers::read(&env),
    }
    .map_err(|e| ServeFault::Config(e.to_string()))?;
    let _running = start_with(&connection, config, &Roots::from_env(&env), seams, callers).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
