//! Serving the bus.

use crate::acp_gate::AcpGate;
use crate::bus::{Gateway, Handler, IndexedSearch, export};
use crate::callers;
use crate::config::IntentdConfig;
use crate::index_files::IndexStore;
use crate::peer::Peers;
use docket_dbus::callers::Identity;
use docket_dbus::{BusConnection, INTENTS_BUS};
use docket_router::{Router, Seams, Watch, acting_role};
use docket_wire::{CallerId, IntentsReply, IntentsRequest, Member};
use porter_daemon::ProcRoot;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use zbus::fdo::{RequestNameFlags, RequestNameReply};

/// Why the daemon stopped serving.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeFault {
    /// The bus name is taken or the connection failed.
    #[error("bus: {0}")]
    Bus(String),
}

fn bus(error: zbus::Error) -> ServeFault {
    ServeFault::Bus(error.to_string())
}

/// The router as the bus hands it requests. After an app's `Index.Reset` or `Index.Push` is done,
/// what it has pushed is kept in `store`, outside the router's lock.
fn handler<S: Seams + 'static>(router: Arc<Router<S>>, store: Option<IndexStore>) -> Handler {
    Arc::new(
        move |caller: CallerId, request: IntentsRequest, watch: Watch| {
            let (router, store) = (router.clone(), store.clone());
            let pushes = matches!(
                request,
                IntentsRequest::IndexPush(_) | IntentsRequest::IndexReset { .. }
            );
            let reply: Pin<Box<dyn Future<Output = IntentsReply> + Send>> = Box::pin(async move {
                let reply = router.handle_watched(&caller, request, watch).await;
                if let (true, IntentsReply::Done, Some(store)) = (pushes, &reply, store) {
                    let (router, app) = (router.clone(), caller.app.name.clone());
                    // A failed join is a panic in the write; the reply stands either way.
                    let _ = tokio::task::spawn_blocking(move || store.save(&*router, &app)).await;
                }
                reply
            });
            reply
        },
    )
}

/// The router's search as the bus asks it: the index, for a caller whose role may search.
fn search_port<S: Seams + 'static>(router: Arc<Router<S>>) -> IndexedSearch {
    Arc::new(move |caller, ask| {
        let role = acting_role(&caller.roles, Member::SearchQuery)?;
        Some(router.search_indexed(role, ask))
    })
}

/// Exports every interface of `org.quire.Intents1` on `connection` over `router` and claims the
/// name; the roles of the callers are `config`'s, and the processes behind them are named by the
/// shipped `intentd-callers.toml` over the system's `/proc`. Returns once the name is ours and the
/// connection keeps serving until it closes.
pub async fn serve_on<S: Seams + 'static>(
    connection: &BusConnection,
    router: Arc<Router<S>>,
    config: Arc<IntentdConfig>,
) -> Result<(), ServeFault> {
    serve_on_with(connection, router, config, &ProcRoot::System).await
}

/// As [`serve_on`], naming callers by the cgroups under `proc_root` (the system's `/proc` unless
/// a test build chose a fixture tree).
pub async fn serve_on_with<S: Seams + 'static>(
    connection: &BusConnection,
    router: Arc<Router<S>>,
    config: Arc<IntentdConfig>,
    proc_root: &ProcRoot,
) -> Result<(), ServeFault> {
    let who = Arc::new(Identity::on(
        connection.clone(),
        callers::shipped(),
        proc_root.fixture(),
    ));
    serve_on_gated(
        connection,
        router,
        config,
        who,
        proc_root,
        AcpGate::shut(),
        None,
    )
    .await
}

/// As [`serve_on_with`], with callers named by `who` (the daemon's table) and `org.quire.Acp` an
/// app only while `acp` shows the setting on. With an `index` store, what each app pushes is kept
/// in it.
pub async fn serve_on_gated<S: Seams + 'static>(
    connection: &BusConnection,
    router: Arc<Router<S>>,
    config: Arc<IntentdConfig>,
    who: Arc<Identity>,
    proc_root: &ProcRoot,
    acp: AcpGate,
    index: Option<IndexStore>,
) -> Result<(), ServeFault> {
    let gateway = Gateway::new(
        handler(router.clone(), index),
        search_port(router),
        Peers::gated(connection.clone(), config, who, proc_root, acp),
    );
    export(connection, &gateway).await.map_err(bus)?;
    // Never queued behind another intentd: two routers would be two breakers and two journals.
    let reply = connection
        .request_name_with_flags(INTENTS_BUS, RequestNameFlags::DoNotQueue.into())
        .await
        .map_err(bus)?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(()),
        RequestNameReply::InQueue | RequestNameReply::Exists => {
            Err(ServeFault::Bus(format!("{INTENTS_BUS} is taken")))
        }
    }
}

/// Claims `org.quire.Intents1` on the session bus and serves every interface of it over
/// `router`, deriving each caller's identity and role from the connection (never from a body)
/// by `config`'s roles, until the connection closes.
pub async fn serve<S: Seams + 'static>(
    router: Arc<Router<S>>,
    config: Arc<IntentdConfig>,
) -> Result<(), ServeFault> {
    let connection = BusConnection::session().await.map_err(bus)?;
    serve_on(&connection, router, config).await?;
    closed(&connection).await;
    Ok(())
}

/// Returns when the connection closes.
pub(crate) async fn closed(connection: &BusConnection) {
    use futures_util::StreamExt;
    let mut messages = zbus::MessageStream::from(connection);
    while messages.next().await.is_some() {}
}
