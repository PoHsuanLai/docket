//! The library behind docket's two agent-facing programs, so the acceptance harness can run them
//! under its own name.
//!
//! - [`serve`], [`LiveFault`], [`BUS_NAME`]: `docket-acp`, the ACP server edge on stdio. It hosts
//!   its own companion over the session bus and serves the editor's connection.
//! - [`entry::docket_agent`]: `docket-agent`, the host that runs one configured coding agent as a
//!   docket session under the launcher's sandbox. The host itself (`agent`) is private.
//! - [`confirm`]: the confirm object the editor's sheets reach the person through.
//!
//! The docket-acp process's live host: what the binary builds so a served connection has a session host. docket-acp
//! reaches docket the way companiond does, over the session bus: the router as `Intents1`
//! (`DbusTransport`), the model through inferd, stored sessions read through the router. It hosts its own
//! `Companion` (`docket_native::NativeHost`), so the editor's reject is heard before a call runs.
//!
//! The router tells who this is by its bus name, `org.quire.Acp`, and `intentd.toml` lists that
//! name under both `editor` (opens sessions and records the editor's turns) and `companion`
//! (the planner's calls in them). Without those lines the router refuses everything this does.

mod agent;
pub mod callers;
pub mod confirm;
pub mod entry;
#[cfg(feature = "testing")]
pub mod harness;
pub mod helpers;

use companion_client::DbusHelpers;
use confirm::ConfirmObject;
use docket_acp::{Permit, Server, capped_lines};
use docket_client::{DbusTransport, Intents};
use docket_core::AgentConfig;
use docket_dbus::callers::Identity;
use docket_dbus::{CONFIRM_PATH, InferLink};
use docket_host::EditorDesk;
use docket_native::NativeHost;
use docket_planner::PlannerModel;
use docket_store::SystemClock;
use docket_tasks::{Companion, Quiet, RouterLog};
use helpers::LiveHelpers;
use porter_callers::ProcRoot;
use porter_core::AppName;
use std::sync::Arc;
use tokio::io::{BufReader, stdin, stdout};

/// The bus name the router knows this process by.
pub const BUS_NAME: &str = "org.quire.Acp";

/// The shell's name, for the companion's `shell` (no launcher is involved in an editor session).
const SHELL: &str = "org.quire.Shell";

type Log = Arc<RouterLog<DbusTransport>>;

/// The host the binary serves with.
pub type LiveHost =
    NativeHost<PlannerModel<InferLink>, DbusTransport, SystemClock, Quiet, Log, EditorDesk>;

/// Why the process could not serve.
#[derive(Debug, thiserror::Error)]
pub enum LiveFault {
    /// The session bus, or the name on it.
    #[error("bus: {0}")]
    Bus(String),
    /// The editor's app name is not one.
    #[error("not an app name: {0}")]
    App(String),
    /// `acp-callers.toml` exists and cannot be used.
    #[error("{0}")]
    Callers(String),
}

/// Serves one editor on stdio until it closes the connection. `trace` is the inferd watch: the
/// packaged binary passes `InferWatch::off()`, only the acceptance harness passes a file.
///
/// The editor's app, which the edge checks a stored session's opener against, is the bus name:
/// the router records the opener and the editor's turns under the app behind the connection.
/// `env` is the environment `main` hands in.
pub async fn serve(
    permit: Permit,
    trace: docket_dbus::InferWatch,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<(), LiveFault> {
    serve_on(permit, trace, Callers::default(), env).await
}

/// Where the callers are read from: the system's `/proc` and callers file, unless the acceptance
/// harness (`harness::acp`) points at a fixture tree and a file of its own.
#[derive(Debug)]
pub(crate) struct Callers {
    pub(crate) proc_root: ProcRoot,
    pub(crate) system_callers: Option<std::path::PathBuf>,
}

impl Default for Callers {
    fn default() -> Self {
        Self {
            proc_root: ProcRoot::Unset,
            system_callers: None,
        }
    }
}

async fn serve_on(
    permit: Permit,
    trace: docket_dbus::InferWatch,
    seams: Callers,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<(), LiveFault> {
    let editor = AppName::parse(BUS_NAME).map_err(|_| LiveFault::App(BUS_NAME.to_owned()))?;
    let bus = |e: zbus::Error| LiveFault::Bus(e.to_string());
    let connection = docket_dbus::session_connection(env).await.map_err(bus)?;
    // The sheets intentd hands this process for an editor's session wait on the desk the host
    // shows the editor. Served before the name is claimed, so the first sheet finds it.
    let desk = EditorDesk::new();
    let table = match &seams.system_callers {
        Some(system) => callers::read_at(system, env),
        None => callers::read(env),
    }
    .map_err(|e| LiveFault::Callers(e.to_string()))?;
    let who = Arc::new(Identity::on(
        connection.clone(),
        table,
        seams.proc_root.honoured().map(std::path::PathBuf::as_path),
    ));
    connection
        .object_server()
        .at(
            CONFIRM_PATH,
            ConfirmObject::new(desk.clone(), SystemClock, who),
        )
        .await
        .map_err(bus)?;
    connection.request_name(BUS_NAME).await.map_err(bus)?;
    let shell = AppName::parse(SHELL).map_err(|_| LiveFault::App(SHELL.to_owned()))?;
    let companion = Companion::new(
        Intents::over(DbusTransport::new(connection.clone())),
        PlannerModel::new(docket_dbus::inferd_transport(&connection, trace)),
        AgentConfig::default(),
        SystemClock,
        shell,
    )
    // An editor's sessions are not the companion's: no roster notes, so a restart of companiond
    // does not list them.
    .without_notes();
    // Stored sessions are read through the router: memoryd answers log reads for the router and
    // the shell only.
    let log: Log = Arc::new(RouterLog::new(Intents::over(DbusTransport::new(
        connection.clone(),
    ))));
    // The person's helpers, if companiond answers this process; the editor goes without them if
    // it does not.
    let talking = DbusHelpers::over(&connection).await.ok().map(|helpers| {
        (
            helpers,
            Intents::over(DbusTransport::new(connection.clone())),
        )
    });
    let helpers = LiveHelpers::over(talking).await;
    let host: LiveHost = NativeHost::over(companion, log.clone()).with_desk(desk);
    let lines = capped_lines(BufReader::new(stdin()), stdout());
    let mut server = Server::new(permit, editor, host, log, SystemClock).with_helpers(helpers);
    // The editor closing the pipe is the normal end.
    let _ = server.run(lines).await;
    Ok(())
}
