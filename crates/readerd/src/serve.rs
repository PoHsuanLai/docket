//! Serving `org.quire.Reader1`.

use crate::service::ReaderService;
use docket_client::Transport as IntentsTransport;
use porter_client::Transport as InferTransport;
use std::sync::Arc;

/// Why the daemon stopped serving.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeFault {
    /// The bus name is taken or the connection failed.
    #[error("bus: {0}")]
    Bus(String),
}

/// Claims `org.quire.Reader1` and serves `Extract(session, ask)` until the connection closes.
/// Only intentd may call it: the caller is checked against the configured bus name before the
/// ask is read.
pub async fn serve<P, I>(service: Arc<ReaderService<P, I>>) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let _ = (
        service,
        docket_dbus::READER_BUS,
        docket_dbus::READER_PATH,
        docket_dbus::ReaderSkeleton,
    );
    todo!(
        "serve: export a ReaderSkeleton replacement at /org/quire/Reader1 whose Extract decodes the ReaderAsk JSON, runs ReaderService::extract_in, answers the Value JSON; refuse any caller but intentd"
    )
}
