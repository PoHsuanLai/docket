//! Serving an app's provider on its own bus name.

use crate::provider::{ContextSource, IntentProvider, SummonTarget};
use crate::transport::TransportError;

/// Serves `org.quire.IntentProvider1` for `provider` on the app's activatable bus name (the
/// manifest's app) on the session bus, until the connection closes. The router calls it only
/// after checking that the name's owner derives to the same `AppId`; every member but `Summon`
/// refuses any caller that is not intentd.
#[cfg(feature = "dbus")]
pub async fn serve<P, C, S>(provider: P, context: C, summon: S) -> Result<(), TransportError>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    S: SummonTarget + 'static,
{
    let connection = docket_dbus::BusConnection::session()
        .await
        .map_err(|e| TransportError::Bus(e.to_string()))?;
    crate::provider_bus::serve_on(&connection, provider, context, summon).await?;
    crate::provider_bus::until_closed(&connection).await;
    Ok(())
}

/// Without the `dbus` feature there is no bus to serve on.
#[cfg(not(feature = "dbus"))]
pub async fn serve<P, C, S>(provider: P, context: C, summon: S) -> Result<(), TransportError>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    S: SummonTarget + 'static,
{
    let _ = (provider, context, summon);
    Err(TransportError::Closed)
}
