//! The docket-acp process's live host: what the binary builds so a served connection has a session host. docket-acp
//! reaches docket the way companiond does, over the session bus: the router as `Intents1`
//! (`DbusTransport`), the model through inferd, stored sessions read through the router. It hosts its own
//! `Companion` (`docket_tasks::NativeHost`), so the editor's reject is heard before a call runs.
//!
//! The router tells who this is by its bus name, `org.quire.Acp`, and `intentd.toml` lists that
//! name under both `editor` (opens sessions and records the editor's turns) and `companion`
//! (the planner's calls in them). Without those lines the router refuses everything this does.

pub mod confirm;

use confirm::ConfirmObject;
use docket_acp::{LineWire, Permit, Server, SystemTicks, Ticks};
use docket_client::{DbusTransport, Intents};
use docket_core::{AgentConfig, Millis};
use docket_dbus::{CONFIRM_PATH, InferLink};
use docket_inapp::EditorDesk;
use docket_planner::PlannerModel;
use docket_router::Clock;
use docket_tasks::{Companion, NativeHost, Now, Quiet, RouterLog};
use porter_core::AppName;
use prov::UnixSeconds;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{BufReader, stdin, stdout};

/// The bus name the router knows this process by.
pub const BUS_NAME: &str = "org.quire.Acp";

/// The shell's name, for the companion's `shell` (no launcher is involved in an editor session).
const SHELL: &str = "org.quire.Shell";

/// The system clock.
#[derive(Debug, Clone, Copy)]
pub struct Wall;

impl Now for Wall {
    fn now(&self) -> UnixSeconds {
        SystemTicks.now()
    }
}

impl Clock for Wall {
    fn now(&self) -> UnixSeconds {
        SystemTicks.now()
    }

    fn after(&self, wait: Millis) -> impl Future<Output = ()> + Send {
        tokio::time::sleep(Duration::from_millis(u64::from(wait.0)))
    }
}

type Log = Arc<RouterLog<DbusTransport>>;

/// The host the binary serves with.
pub type LiveHost = NativeHost<InferLink, DbusTransport, Wall, Quiet, Log, EditorDesk>;

/// Why the process could not serve.
#[derive(Debug, thiserror::Error)]
pub enum LiveFault {
    /// The session bus, or the name on it.
    #[error("bus: {0}")]
    Bus(String),
    /// The editor's app name is not one.
    #[error("not an app name: {0}")]
    App(String),
}

/// Serves one editor on stdio until it closes the connection.
///
/// The editor's app, which the edge checks a stored session's opener against, is the bus name:
/// the router records the opener and the editor's turns under the app behind the connection.
pub async fn serve(permit: Permit) -> Result<(), LiveFault> {
    let editor = AppName::parse(BUS_NAME).map_err(|_| LiveFault::App(BUS_NAME.to_owned()))?;
    let env = |key: &str| std::env::var(key).ok();
    let bus = |e: zbus::Error| LiveFault::Bus(e.to_string());
    let connection = docket_dbus::session_connection(&env).await.map_err(bus)?;
    // The sheets intentd hands this process for an editor's session wait on the desk the host
    // shows the editor. Served before the name is claimed, so the first sheet finds it.
    let desk = EditorDesk::new();
    connection
        .object_server()
        .at(CONFIRM_PATH, ConfirmObject::new(desk.clone(), Wall))
        .await
        .map_err(bus)?;
    connection.request_name(BUS_NAME).await.map_err(bus)?;
    let shell = AppName::parse(SHELL).map_err(|_| LiveFault::App(SHELL.to_owned()))?;
    let companion = Companion::new(
        Intents::over(DbusTransport::new(connection.clone())),
        PlannerModel::new(docket_dbus::inferd_transport(&connection)),
        AgentConfig::default(),
        Wall,
        shell,
    );
    // Stored sessions are read through the router: memoryd answers log reads for the router and
    // the shell only.
    let log: Log = Arc::new(RouterLog::new(Intents::over(DbusTransport::new(
        connection,
    ))));
    let host: LiveHost = NativeHost::over(companion, log.clone()).with_desk(desk);
    let wire = LineWire::new(BufReader::new(stdin()), stdout());
    let mut server = Server::new(permit, editor, host, log, wire, SystemTicks);
    // The editor closing the pipe is the normal end.
    let _ = server.run().await;
    Ok(())
}
