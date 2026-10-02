//! Serving `org.quire.Companion1`.

use crate::runtime::Companiond;
use docket_client::Transport as IntentsTransport;
use porter_client::Transport as InferTransport;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Why the daemon stopped serving.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeFault {
    /// The bus name is taken or the connection failed.
    #[error("bus: {0}")]
    Bus(String),
}

/// Claims `org.quire.Companion1`, serves the root object and one answer object per task
/// (`/org/quire/Companion1/answer/<task>`), and runs until the connection closes.
pub async fn serve<P, I>(companion: Arc<Mutex<Companiond<P, I>>>) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let _ = (
        companion,
        docket_dbus::COMPANION_BUS,
        docket_dbus::CompanionSkeleton,
        docket_dbus::CompanionAnswerSkeleton,
    );
    todo!(
        "serve: export the Companion1 and Companion1.Answer handlers; on startup call recover and open a fresh session for the front task; Message.Arrived triggers Companiond::arrived; the answer objects' Updated signals carry AnswerWire"
    )
}
