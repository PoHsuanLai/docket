//! The daemon: configuration, the session bus, the router and inferd over it, a restart that
//! rebuilds the roster and the front task from the eventlog, and `Companion1` served.

use crate::Companiond;
use crate::apps_link::AppsLink;
use crate::config::CompaniondConfig;
use crate::handoff::HostLink;
use crate::serve::serve_with_routines;
use companion_model::helper::{AgentApps, HelperCard};
use docket_client::{DbusTransport, Intents};
use docket_dbus::InferWatch;
use docket_dbus::callers::{CallerTable, Identity};
use docket_dbus::{BusConnection, InferLink};
use docket_helpers::{Hub, HubHost, RoutineDesk, Routines, Timers, TokioSpawn};
use docket_planner::PlannerModel;
use docket_skills::{Roots, discover};
use docket_skills_disk::DiskFolder;
use docket_store::SystemClock;
use docket_tasks::ServeFault;
use futures_util::StreamExt;
use futures_util::lock::Mutex;
use porter_daemon::ProcRoot;
use std::path::PathBuf;
use std::sync::Arc;

/// What only the acceptance harness's mains choose: the `/proc` callers are read from (a tree
/// other than `/proc` is a fixture, whose units' main processes are in `<root>/units`), the
/// system's callers file, and whether the inferd watch writes to a file. The shipped `main` passes
/// the default (the system's `/proc`, `/etc/quire`, no watch) and no environment variable can
/// change any.
#[derive(Debug, Clone)]
pub struct HarnessSeams {
    /// Where callers' cgroups are read.
    pub proc_root: ProcRoot,
    /// The system's callers file, when the harness's main chose another than `/etc/quire`'s: the
    /// harness is its own system, and a fixed role (the shell's) comes from no other file.
    pub system_callers: Option<PathBuf>,
    /// What is kept of the exchanges with inferd.
    pub trace: InferWatch,
}

impl Default for HarnessSeams {
    fn default() -> Self {
        Self {
            proc_root: ProcRoot::Unset,
            system_callers: None,
            trace: InferWatch::off(),
        }
    }
}

/// The companion as the daemon runs it: inferd and the router over the session bus.
pub type Daemon = Companiond<PlannerModel<InferLink>, DbusTransport>;

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
    start_with_agents(connection, config, skills, seams, callers, None).await
}

/// [`start_with`] with the host of the agent apps that are helpers: a helper whose kind is an
/// agent app runs its jobs there, one agent process per helper. With none, such a job fails at
/// once, and every other helper is unaffected.
pub async fn start_with_agents(
    connection: &BusConnection,
    config: CompaniondConfig,
    skills: &Roots,
    seams: HarnessSeams,
    callers: CallerTable,
    agents: Option<Arc<dyn AgentApps>>,
) -> Result<Arc<Mutex<Daemon>>, ServeFault> {
    let found = discover(skills, &DiskFolder);
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
        config.agent,
        SystemClock,
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
        let (agent, shell) = (config.agent, config.shell.clone());
        let make = move |card: &HelperCard| {
            Companiond::new(
                Intents::over(DbusTransport::new(link.clone())),
                PlannerModel::new(docket_dbus::inferd_transport(&link, trace.clone()))
                    .with_open_options(docket_tasks::open_options(card)),
                agent,
                SystemClock,
                shell.clone(),
            )
        };
        // Reads and writes the helpers' records; it never asks a model.
        let service = Companiond::new(
            Intents::over(DbusTransport::new(connection.clone())),
            PlannerModel::new(docket_dbus::inferd_transport(connection, InferWatch::off())),
            config.agent,
            SystemClock,
            config.shell.clone(),
        );
        Hub::new(
            service,
            companion.shared.clone(),
            Arc::new(TokioSpawn),
            make,
        )
    };
    if let Some(agents) = agents {
        hub.set_agents(agents);
    }
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
    // The helpers' routines: a small timer here runs them. The systemd timers are not written until
    // the unit's sandbox lets the daemon reach the user's unit directory (`Timers::Dir`).
    let routines = Arc::new(Routines::new(
        Arc::new(HubHost::new(hub.clone(), Timers::Off)),
        SystemClock,
    ));
    if tokio::time::timeout(companion.waits.startup, routines.restore(&config.spaces))
        .await
        .is_err()
    {
        tracing::warn!("the routines did not come back in time; starting without them");
    }
    tokio::spawn(routines.clone().run());
    let companion = Arc::new(Mutex::new(companion));
    let who = Arc::new(Identity::on(
        connection.clone(),
        callers,
        seams.proc_root.honoured().map(std::path::PathBuf::as_path),
    ));
    let kept: Arc<dyn RoutineDesk> = routines;
    serve_with_routines(
        connection,
        companion.clone(),
        who,
        seams.proc_root,
        Some(kept),
    )
    .await?;
    Ok(companion)
}

/// The daemon: its configuration and the session bus from `env` (the process's environment, which
/// its `main` passes in), served until the bus closes.
pub async fn run(env: &impl Fn(&str) -> Option<String>) -> Result<(), ServeFault> {
    run_with(HarnessSeams::default(), env).await
}

/// [`run`] with the seams the acceptance harness's mains choose.
pub async fn run_with(
    seams: HarnessSeams,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<(), ServeFault> {
    let config = CompaniondConfig::from_env(env).map_err(|e| ServeFault::Config(e.to_string()))?;
    let connection = docket_dbus::session_connection(env)
        .await
        .map_err(|e| ServeFault::Bus(e.to_string()))?;
    let callers = match &seams.system_callers {
        Some(system) => crate::callers::read_at(system, env),
        None => crate::callers::read(env),
    }
    .map_err(|e| ServeFault::Config(e.to_string()))?;
    // The agent apps that are helpers run in the host the bus starts (`docket-agent --serve`); the
    // tests that start a daemon of their own pass a scripted host or none (`start_with_agents`).
    let agents: Arc<dyn AgentApps> = Arc::new(AppsLink::new(connection.clone()));
    let _running = start_with_agents(
        &connection,
        config,
        &Roots::from_env(env),
        seams,
        callers,
        Some(agents),
    )
    .await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
