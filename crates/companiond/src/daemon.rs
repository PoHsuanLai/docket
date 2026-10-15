//! The daemon: configuration, the session bus, the router and inferd over it, a restart that
//! rebuilds the roster and the front task from the eventlog, and `Companion1` served.

use crate::Companiond;
use crate::clock::Clock;
use crate::config::CompaniondConfig;
use crate::serve::serve_on_rooted;
use docket_client::{DbusTransport, Intents};
use docket_dbus::tap::Tap;
use docket_dbus::{BusConnection, InferLink};
use docket_planner::PlannerModel;
use docket_skills::{Roots, discover};
use docket_tasks::ServeFault;
use futures_util::StreamExt;
use futures_util::lock::Mutex;
use std::path::PathBuf;
use std::sync::Arc;

/// What only the acceptance harness's mains choose: the `/proc` callers are read from and
/// whether the model tap writes to a file. The shipped `main` passes the default (the system's
/// `/proc`, no tap) and no environment variable can change either.
#[derive(Debug, Clone)]
pub struct HarnessSeams {
    /// Where callers' cgroups are read.
    pub proc_root: PathBuf,
    /// What is kept of the exchanges with inferd.
    pub trace: Tap,
}

impl Default for HarnessSeams {
    fn default() -> Self {
        Self {
            proc_root: PathBuf::from("/proc"),
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
) -> Result<Arc<Mutex<Daemon>>, ServeFault> {
    start_with(
        connection,
        config,
        &Roots::default(),
        HarnessSeams::default(),
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
) -> Result<Arc<Mutex<Daemon>>, ServeFault> {
    let found = discover(skills);
    for rejected in &found.rejected {
        eprintln!(
            "companiond: skill {}: {}",
            rejected.dir.display(),
            rejected.fault
        );
    }
    let intents = Intents::over(DbusTransport::new(connection.clone()));
    let planner = PlannerModel::new(docket_dbus::inferd_transport(connection, seams.trace));
    let mut companion = Companiond::new(
        intents,
        planner,
        config.agent,
        Clock::System,
        config.shell.clone(),
    )
    .with_skills(found.skills);
    // A router or memoryd that hangs must not keep the name from being claimed.
    match tokio::time::timeout(companion.waits.startup, companion.restore(&config.spaces)).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => eprintln!("companiond: restart found nothing to resume: {error}"),
        Err(_) => eprintln!("companiond: the restart did not finish in time; starting without it"),
    }
    let companion = Arc::new(Mutex::new(companion));
    serve_on_rooted(connection, companion.clone(), seams.proc_root).await?;
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
    let _running = start_with(&connection, config, &Roots::from_env(&env), seams).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
