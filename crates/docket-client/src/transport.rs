//! How a request reaches the router.

use crate::watch::Watched;
#[cfg(feature = "in_process")]
use docket_core::CallerId;
use docket_core::{IntentsReply, IntentsRequest};
#[cfg(feature = "in_process")]
use docket_router::{Router, Seams};
use std::future::Future;
#[cfg(feature = "in_process")]
use std::sync::Arc;

/// Why a request got no reply.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum TransportError {
    /// The connection is closed or the daemon is not there.
    #[error("connection closed")]
    Closed,
    /// The other side answered something that is not the protocol.
    #[error("malformed message: {0}")]
    Malformed(String),
    /// The bus refused or failed.
    #[error("bus: {0}")]
    Bus(String),
}

/// One carrier of the Intents1 protocol.
pub trait Transport: Send + Sync {
    /// Sends one request and waits for its reply.
    fn call(
        &self,
        request: IntentsRequest,
    ) -> impl Future<Output = Result<IntentsReply, TransportError>> + Send;

    /// Sends one request and watches it: its progress comes out as it happens and the caller may
    /// `proceed` or `close` it. A transport that cannot watch sends the request as `call` does
    /// and gives the answer alone.
    fn watch(
        &self,
        request: IntentsRequest,
    ) -> impl Future<Output = Result<Watched, TransportError>> + Send {
        async move { Ok(Watched::answered(self.call(request).await?)) }
    }
}

/// The router in the caller's own process, as one fixed caller. For tests and for a host that
/// embeds the core.
#[cfg(feature = "in_process")]
#[derive(Debug)]
pub struct InProcess<S: Seams> {
    router: Arc<Router<S>>,
    caller: CallerId,
}

#[cfg(feature = "in_process")]
impl<S: Seams> InProcess<S> {
    /// Talks to `router` as `caller`.
    pub fn new(router: Arc<Router<S>>, caller: CallerId) -> Self {
        Self { router, caller }
    }
}

#[cfg(feature = "in_process")]
impl<S: Seams + 'static> Transport for InProcess<S> {
    async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
        Ok(self.router.handle(&self.caller, request).await)
    }

    async fn watch(&self, request: IntentsRequest) -> Result<Watched, TransportError> {
        Ok(crate::watch_in_process::watched(
            self.router.clone(),
            self.caller.clone(),
            request,
        ))
    }
}

/// The desktop's intentd over D-Bus.
#[cfg(feature = "dbus")]
#[derive(Debug)]
pub struct DbusTransport {
    connection: docket_dbus::BusConnection,
}

#[cfg(feature = "dbus")]
impl DbusTransport {
    /// Talks to intentd over `connection`.
    pub fn new(connection: docket_dbus::BusConnection) -> Self {
        Self { connection }
    }

    /// Connects to the session bus and makes sure intentd is there: already running, or started
    /// through D-Bus activation when it is installed and not running. A bus with no intentd on
    /// it, installed or running, is `Closed`: the caller says "unavailable", and no request was
    /// sent. (A name that already has an owner is not activated: a bus without an activation
    /// file for it, a private bus a person started intentd on by hand, answers
    /// `StartServiceByName` with an error even then.)
    pub async fn connect() -> Result<Self, TransportError> {
        let connection = docket_dbus::BusConnection::session()
            .await
            .map_err(|e| TransportError::Bus(e.to_string()))?;
        let running = connection
            .call_method(
                Some("org.freedesktop.DBus"),
                "/org/freedesktop/DBus",
                Some("org.freedesktop.DBus"),
                "NameHasOwner",
                &(docket_dbus::INTENTS_BUS,),
            )
            .await
            .map_err(|e| TransportError::Bus(e.to_string()))?
            .body()
            .deserialize::<bool>()
            .map_err(|e| TransportError::Bus(e.to_string()))?;
        if !running {
            connection
                .call_method(
                    Some("org.freedesktop.DBus"),
                    "/org/freedesktop/DBus",
                    Some("org.freedesktop.DBus"),
                    "StartServiceByName",
                    &(docket_dbus::INTENTS_BUS, 0u32),
                )
                .await
                .map_err(|_| TransportError::Closed)?;
        }
        Ok(Self::new(connection))
    }
}

#[cfg(feature = "dbus")]
impl Transport for DbusTransport {
    async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
        crate::bus::call(&self.connection, request).await
    }

    async fn watch(&self, request: IntentsRequest) -> Result<Watched, TransportError> {
        crate::bus::watch(&self.connection, request).await
    }
}
