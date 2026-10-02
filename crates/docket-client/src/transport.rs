//! How a request reaches the router.

use docket_core::{CallerId, IntentsReply, IntentsRequest};
use docket_router::{Router, Seams};
use std::future::Future;
use std::sync::Arc;

/// Why a request got no reply.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
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
}

/// The router in the caller's own process, as one fixed caller. For tests and for a host that
/// embeds the core.
#[derive(Debug)]
pub struct InProcess<S: Seams> {
    router: Arc<Router<S>>,
    caller: CallerId,
}

impl<S: Seams> InProcess<S> {
    /// Talks to `router` as `caller`.
    pub fn new(router: Arc<Router<S>>, caller: CallerId) -> Self {
        Self { router, caller }
    }
}

impl<S: Seams> Transport for InProcess<S> {
    async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
        Ok(self.router.handle(&self.caller, request).await)
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
}

#[cfg(feature = "dbus")]
impl Transport for DbusTransport {
    async fn call(&self, request: IntentsRequest) -> Result<IntentsReply, TransportError> {
        let _ = (&self.connection, request);
        todo!(
            "DbusTransport::call: one match from IntentsRequest to the member and its JSON arguments; a bus error name maps back to WireRefusal through IntentsError; the Request object's Response signal carries the reply of Perform, Undo, Widen and Check"
        )
    }
}
