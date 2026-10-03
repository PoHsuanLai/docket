//! Serving `org.quire.Reader1`.

use crate::service::ReaderService;
use docket_client::Transport as IntentsTransport;
use docket_core::{ReaderAsk, ReaderError, Value};
use docket_dbus::{BusConnection, Details, INTENTS_BUS, READER_BUS, READER_PATH};
use porter_client::Transport as InferTransport;
use prov::SessionId;
use std::sync::Arc;
use zbus::fdo::{self, DBusProxy, RequestNameFlags, RequestNameReply};
use zbus::message::Header;
use zbus::names::BusName;

/// Why the daemon stopped serving.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeFault {
    /// The bus name is taken or the connection failed.
    #[error("bus: {0}")]
    Bus(String),
}

fn bus(error: zbus::Error) -> ServeFault {
    ServeFault::Bus(error.to_string())
}

/// `org.quire.Reader1` over a service.
struct ReaderObject<P: InferTransport, I: IntentsTransport> {
    service: Arc<ReaderService<P, I>>,
    connection: BusConnection,
}

impl<P: InferTransport, I: IntentsTransport> std::fmt::Debug for ReaderObject<P, I> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReaderObject")
    }
}

impl<P: InferTransport, I: IntentsTransport> ReaderObject<P, I> {
    /// Whether `sender` is the connection that owns intentd's name right now: the only caller
    /// the reader answers.
    async fn is_intentd(&self, sender: &str) -> bool {
        let Ok(proxy) = DBusProxy::new(&self.connection).await else {
            return false;
        };
        let Ok(name) = BusName::try_from(INTENTS_BUS) else {
            return false;
        };
        match proxy.get_name_owner(name).await {
            Ok(owner) => owner.as_str() == sender,
            Err(_) => false,
        }
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
        let sender = header.sender().map(ToString::to_string).unwrap_or_default();
        if !self.is_intentd(&sender).await {
            return Err(fdo::Error::AccessDenied(
                "only intentd may ask the reader".into(),
            ));
        }
        let session = SessionId::parse(&session)
            .map_err(|_| fdo::Error::InvalidArgs("not a session id".into()))?;
        let ask: ReaderAsk = serde_json::from_str(&ask)
            .map_err(|_| fdo::Error::InvalidArgs("not a reader ask".into()))?;
        let answer: Result<Value, ReaderError> = self.service.extract_in(&session, ask).await;
        serde_json::to_string(&answer).map_err(|e| fdo::Error::Failed(e.to_string()))
    }
}

/// Exports `org.quire.Reader1` at its path on `connection` and claims the name. Returns once
/// the name is ours; the connection keeps serving until it closes.
pub async fn serve_on<P, I>(
    connection: &BusConnection,
    service: Arc<ReaderService<P, I>>,
) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let object = ReaderObject {
        service,
        connection: connection.clone(),
    };
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
/// connection closes. Only intentd may call it: the caller is checked against the owner of
/// intentd's name before the ask is read.
pub async fn serve<P, I>(service: Arc<ReaderService<P, I>>) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    use futures_util::StreamExt;
    let connection = BusConnection::session().await.map_err(bus)?;
    serve_on(&connection, service).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
