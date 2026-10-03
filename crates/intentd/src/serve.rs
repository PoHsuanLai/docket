//! Serving the bus.

use crate::bus::{Gateway, Handler, export};
use crate::config::IntentdConfig;
use crate::peer::Peers;
use docket_core::{CallerId, IntentsReply, IntentsRequest};
use docket_dbus::{BusConnection, INTENTS_BUS};
use docket_router::{Router, Seams};
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

/// The router as the bus hands it requests.
fn handler<S: Seams + 'static>(router: Arc<Router<S>>) -> Handler {
    Arc::new(move |caller: CallerId, request: IntentsRequest| {
        let router = router.clone();
        let reply: Pin<Box<dyn Future<Output = IntentsReply> + Send>> =
            Box::pin(async move { router.handle(&caller, request).await });
        reply
    })
}

/// Exports every interface of `org.quire.Intents1` on `connection` over `router` and claims the
/// name; the roles of the callers are `config`'s. Returns once the name is ours and the
/// connection keeps serving until it closes.
pub async fn serve_on<S: Seams + 'static>(
    connection: &BusConnection,
    router: Arc<Router<S>>,
    config: Arc<IntentdConfig>,
) -> Result<(), ServeFault> {
    let gateway = Gateway::new(handler(router), Peers::new(connection.clone(), config));
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
/// `router`, deriving each caller's identity and role from the connection (never from a body),
/// until the connection closes. The roles are the shipped defaults of `IntentdConfig`; a
/// daemon with a file of its own calls [`serve_on`].
pub async fn serve<S: Seams + 'static>(router: Arc<Router<S>>) -> Result<(), ServeFault> {
    let connection = BusConnection::session().await.map_err(bus)?;
    let config = IntentdConfig::shipped().map_err(|e| ServeFault::Bus(e.to_string()))?;
    serve_on(&connection, router, Arc::new(config)).await?;
    closed(&connection).await;
    Ok(())
}

/// Returns when the connection closes.
pub(crate) async fn closed(connection: &BusConnection) {
    use futures_util::StreamExt;
    let mut messages = zbus::MessageStream::from(connection);
    while messages.next().await.is_some() {}
}
