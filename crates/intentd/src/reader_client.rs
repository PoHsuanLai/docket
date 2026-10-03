//! The quarantined reader, over `org.quire.Reader1`.

use docket_core::{Reader, ReaderAsk, ReaderError, Value};
use docket_dbus::{BusConnection, Details, ReaderProxy};
use prov::{Quarantined, SessionId};

/// The quarantined reader, over `org.quire.Reader1`.
#[derive(Debug, Clone)]
pub struct ReaderClient {
    connection: BusConnection,
}

impl ReaderClient {
    /// Calls readerd over `connection`.
    pub fn new(connection: BusConnection) -> Self {
        Self { connection }
    }

    /// `Reader1.Extract(session, ask)`: readerd resolves the ask's handles in `session` through
    /// `Session.Resolve` (the inputs stay in intentd's handle table) and answers a value that
    /// fits the schema, as the JSON of `Result<Value, ReaderError>`. A reader that is not on the
    /// bus is `ModelUnavailable`: never a value, never a panic.
    pub async fn extract_in(
        &self,
        session: &SessionId,
        ask: &ReaderAsk,
    ) -> Result<Value, ReaderError> {
        let ask = serde_json::to_string(ask).map_err(|_| ReaderError::Unparseable)?;
        let proxy = ReaderProxy::new(&self.connection)
            .await
            .map_err(|_| ReaderError::ModelUnavailable)?;
        let answer = proxy
            .extract(session.as_str(), &ask, &Details::new())
            .await
            .map_err(|_| ReaderError::ModelUnavailable)?;
        serde_json::from_str::<Result<Value, ReaderError>>(&answer)
            .map_err(|_| ReaderError::Unparseable)?
    }
}

impl Reader for ReaderClient {
    /// The seam hands over the quarantined text too, but `Reader1.Extract` names the session
    /// instead: readerd resolves the handles itself, so the text never crosses the bus.
    async fn extract(
        &self,
        session: &SessionId,
        ask: ReaderAsk,
        _inputs: Vec<Quarantined<String>>,
    ) -> Result<Value, ReaderError> {
        self.extract_in(session, &ask).await
    }
}
