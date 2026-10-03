//! The daemon: the session bus, an inferd link and the router's `Intents1` as the reader (its
//! connection owns `org.quire.Reader1`, which intentd's configuration names as the reader role),
//! and the one `ReaderHost` of the process.

use crate::host::ReaderHost;
use crate::serve::{ServeFault, serve_on};
use crate::service::ReaderService;
use docket_client::{DbusTransport, Intents};
use docket_dbus::BusConnection;
use futures_util::StreamExt;
use std::sync::Arc;

/// Serves `org.quire.Reader1` on `connection` over the inferd and intentd it reaches through it.
/// Returns once the name is ours; the connection keeps serving until it closes.
pub async fn start(connection: &BusConnection) -> Result<(), ServeFault> {
    let intents = Intents::over(DbusTransport::new(connection.clone()));
    let service = ReaderService::on_bus(ReaderHost::start(), connection, intents);
    serve_on(connection, Arc::new(service)).await
}

/// The daemon: the session bus from the environment, served until it closes.
pub async fn run() -> Result<(), ServeFault> {
    let env = |key: &str| std::env::var(key).ok();
    let connection = docket_dbus::session_connection(&env)
        .await
        .map_err(|e| ServeFault::Bus(e.to_string()))?;
    start(&connection).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
