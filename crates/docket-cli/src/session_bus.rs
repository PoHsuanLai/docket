//! The desktop's buses: intentd and the assistant, both on the session bus.

use crate::program::Buses;
use companion_client::DbusCompanion;
use docket_client::{DbusTransport, Intents, TransportError};

/// The session bus.
#[derive(Debug, Clone, Copy)]
pub struct SessionBus;

impl Buses for SessionBus {
    type Intents = DbusTransport;
    type Companion = DbusCompanion;

    async fn intents(&self) -> Result<Intents<DbusTransport>, TransportError> {
        DbusTransport::connect().await.map(Intents::over)
    }

    async fn companion(&self) -> Result<DbusCompanion, TransportError> {
        DbusCompanion::connect().await
    }
}
