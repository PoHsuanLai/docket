//! Serving the bus.

use docket_router::{Router, Seams};
use std::sync::Arc;

/// Why the daemon stopped serving.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeFault {
    /// The bus name is taken or the connection failed.
    #[error("bus: {0}")]
    Bus(String),
}

/// Claims `org.quire.Intents1` and serves every interface of it over `router`, deriving each
/// caller's identity and role from the connection (never from a body), until the connection
/// closes.
pub async fn serve<S: Seams + 'static>(router: Arc<Router<S>>) -> Result<(), ServeFault> {
    let _ = router;
    todo!(
        "serve: build the object server with one handler per Intents1 interface that decodes the member's JSON, calls Router::handle with the derived CallerId, and answers; Request objects for Perform, Undo, Widen and Check; the signals of docket-dbus"
    )
}
