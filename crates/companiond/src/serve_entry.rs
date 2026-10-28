//! The entry points that claim `org.quire.Companion1`: on a connection the caller has (`serve_on`,
//! `serve_on_rooted`), or on the session bus (`serve`). The objects they serve are `serve`'s.

use crate::Companiond;
use crate::serve::serve_with_routines;
use docket_client::Transport as IntentsTransport;
use docket_core::Clock;
use docket_dbus::BusConnection;
use docket_dbus::callers::Identity;
use docket_planner::Planner;
use docket_tasks::ServeFault;
use futures_util::lock::Mutex;
use porter_daemon::ProcRoot;
use std::sync::Arc;

/// Claims `org.quire.Companion1` on `connection`, serves the root object and one answer object
/// per task, listens for `Message.Arrived` and keeps time. The callers are told apart by `who`
/// (the shell is the process it names as the shell's unit; a terminal's child is read from the
/// system's `/proc`). Returns once the name is ours; the connection keeps serving until it closes.
pub async fn serve_on<P, I, K>(
    connection: &BusConnection,
    companion: Arc<Mutex<Companiond<P, I, K>>>,
    who: Arc<Identity>,
) -> Result<(), ServeFault>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
{
    serve_on_rooted(connection, companion, who, ProcRoot::System).await
}

/// [`serve_on`] reading terminals' cgroups under `proc_root` instead of `/proc`: how a test
/// presents its processes as a terminal. The daemon's own `serve_on` always reads `/proc`.
pub async fn serve_on_rooted<P, I, K>(
    connection: &BusConnection,
    companion: Arc<Mutex<Companiond<P, I, K>>>,
    who: Arc<Identity>,
    proc_root: ProcRoot,
) -> Result<(), ServeFault>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
{
    serve_with_routines(connection, companion, who, proc_root, None).await
}

/// Claims `org.quire.Companion1` on the session bus and serves it until the connection closes,
/// telling callers apart by `companiond-callers.toml` (the system's under `/etc/quire`, the
/// person's over it).
pub async fn serve<P, I, K>(
    companion: Arc<Mutex<Companiond<P, I, K>>>,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<(), ServeFault>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
{
    use futures_util::StreamExt;
    let connection = BusConnection::session()
        .await
        .map_err(|e| ServeFault::Bus(e.to_string()))?;
    let table = crate::callers::read(env).map_err(|e| ServeFault::Config(e.to_string()))?;
    let who = Arc::new(Identity::on_system(connection.clone(), table));
    serve_on(&connection, companion, who).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
