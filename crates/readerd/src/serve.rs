//! Serving `org.quire.Reader1`.

use crate::service::ReaderService;
use docket_client::Transport as IntentsTransport;
use docket_core::{ReaderAsk, ReaderError, Value};
use docket_dbus::callers::{INTENTD, Identity, require};
use docket_dbus::{BusConnection, Details, READER_BUS, READER_PATH};
use porter_client::Transport as InferTransport;
use prov::SessionId;
use std::sync::Arc;
use zbus::fdo::{self, RequestNameFlags, RequestNameReply};
use zbus::message::Header;

/// Why the daemon stopped serving.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeFault {
    /// The bus name is taken or the connection failed.
    #[error("bus: {0}")]
    Bus(String),
    /// `readerd-callers.toml` exists and cannot be used.
    #[error("{0}")]
    Callers(String),
}

fn bus(error: zbus::Error) -> ServeFault {
    ServeFault::Bus(error.to_string())
}

/// `org.quire.Reader1` over a service.
struct ReaderObject<P: InferTransport, I: IntentsTransport> {
    service: Arc<ReaderService<P, I>>,
    callers: Arc<Identity>,
}

impl<P: InferTransport, I: IntentsTransport> std::fmt::Debug for ReaderObject<P, I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderObject")
    }
}

#[zbus::interface(name = "org.quire.Reader1")]
impl<P: InferTransport + 'static, I: IntentsTransport + 'static> ReaderObject<P, I> {
    async fn extract(
        &self,
        session: String,
        ask: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let _ = options;
        let sender = header.sender().map(|s| s.as_str());
        if let Err(refused) = require(&self.callers, sender, &[INTENTD]).await {
            tracing::warn!(%refused, "refused a caller that is not intentd");
            return Err(refused.into());
        }
        let session = SessionId::parse(&session).map_err(|_| {
            tracing::warn!("refused a request whose session is not an id");
            fdo::Error::InvalidArgs("not a session id".into())
        })?;
        let ask: ReaderAsk = serde_json::from_str(&ask).map_err(|_| {
            tracing::warn!("refused a request that is not a reader ask");
            fdo::Error::InvalidArgs("not a reader ask".into())
        })?;
        let answer: Result<Value, ReaderError> = self.service.extract_in(&session, ask).await;
        // Which way it failed, never what was read or said.
        if let Err(error) = &answer {
            let reason = match error {
                ReaderError::OutOfSchema(_) => "out_of_schema",
                ReaderError::Unparseable => "unparseable",
                ReaderError::Refused => "refused",
                ReaderError::ModelUnavailable => "model_unavailable",
            };
            tracing::warn!(reason, "the reader gave no answer");
        }
        serde_json::to_string(&answer).map_err(|e| fdo::Error::Failed(e.to_string()))
    }
}

/// Exports `org.quire.Reader1` at its path on `connection` and claims the name. Only the process
/// that `callers` names as intentd (the `agent` role of the app `org.quire.Intents`) may ask.
/// Returns once the name is ours; the connection keeps serving until it closes.
pub async fn serve_on<P, I>(
    connection: &BusConnection,
    service: Arc<ReaderService<P, I>>,
    callers: Arc<Identity>,
) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let object = ReaderObject { service, callers };
    connection
        .object_server()
        .at(READER_PATH, object)
        .await
        .map_err(bus)?;
    let reply = connection
        .request_name_with_flags(READER_BUS, RequestNameFlags::DoNotQueue.into())
        .await
        .map_err(bus)?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => Ok(()),
        RequestNameReply::InQueue | RequestNameReply::Exists => {
            Err(ServeFault::Bus(format!("{READER_BUS} is taken")))
        }
    }
}

/// Claims `org.quire.Reader1` on the session bus and serves `Extract(session, ask)` until the
/// connection closes. Only intentd may call it: the process behind the caller is named by
/// `readerd-callers.toml` (the system's under `/etc/quire`, the person's over it) before the ask
/// is read.
pub async fn serve<P, I>(
    service: Arc<ReaderService<P, I>>,
    env: &impl Fn(&str) -> Option<String>,
) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    use futures_util::StreamExt;
    let connection = BusConnection::session().await.map_err(bus)?;
    let table = crate::callers::read(env).map_err(|e| ServeFault::Callers(e.to_string()))?;
    let who = Arc::new(Identity::on_system(connection.clone(), table));
    serve_on(&connection, service, who).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
