//! The daemon: the session bus, an inferd link and the router's `Intents1` as the reader (its
//! unit is `readerd.service`, which intentd's callers file names as the reader), and the one
//! `ReaderHost` of the process.

use crate::callers;
use crate::host::ReaderHost;
use crate::serve::{ServeFault, serve_on};
use crate::service::ReaderService;
use docket_client::{DbusTransport, Intents};
use docket_dbus::BusConnection;
use docket_dbus::InferWatch;
use docket_dbus::callers::Identity;
use futures_util::StreamExt;
use porter_daemon::ProcRoot;
use std::path::PathBuf;
use std::sync::Arc;

/// Serves `org.quire.Reader1` on `connection` over the inferd and intentd it reaches through it,
/// answering the process `who` names as intentd. Returns once the name is ours; the connection
/// keeps serving until it closes.
pub async fn start(
    connection: &BusConnection,
    tap: InferWatch,
    who: Arc<Identity>,
) -> Result<(), ServeFault> {
    let intents = Intents::over(DbusTransport::new(connection.clone()));
    let service = ReaderService::on_bus(ReaderHost::start(), connection, intents, tap);
    serve_on(connection, Arc::new(service), who).await
}

/// What only the acceptance harness's mains choose: the `/proc` callers are read from and the
/// system's callers file. The shipped `main` passes the default (the system's `/proc`, the file
/// under `/etc/quire`) and no environment variable can change either.
#[derive(Debug, Clone)]
pub struct HarnessSeams {
    /// Where callers' processes are read: the system's `/proc`, or a fixture tree.
    pub proc_root: ProcRoot,
    /// The system's callers file, else `/etc/quire/readerd-callers.toml`.
    pub system_callers: Option<PathBuf>,
}

impl Default for HarnessSeams {
    fn default() -> Self {
        Self {
            proc_root: ProcRoot::Unset,
            system_callers: None,
        }
    }
}

/// The daemon: the session bus from `env` (the process's environment, which its `main` passes
/// in), served until it closes.
pub async fn run(env: &impl Fn(&str) -> Option<String>) -> Result<(), ServeFault> {
    run_with(InferWatch::off(), HarnessSeams::default(), env).await
}

/// [`run`] with the inferd watch and the seams the acceptance harness's mains choose.
pub async fn run_with(
    tap: InferWatch,
    seams: HarnessSeams,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<(), ServeFault> {
    let connection = docket_dbus::session_connection(env)
        .await
        .map_err(|e| ServeFault::Bus(e.to_string()))?;
    let table = match &seams.system_callers {
        Some(system) => callers::read_at(system, env),
        None => callers::read(env),
    }
    .map_err(|e| ServeFault::Callers(e.to_string()))?;
    let who = Arc::new(Identity::on(
        connection.clone(),
        table,
        seams.proc_root.honoured().map(std::path::PathBuf::as_path),
    ));
    start(&connection, tap, who).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
