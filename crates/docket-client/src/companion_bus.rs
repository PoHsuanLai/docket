//! `DbusCompanion`: [`CompanionTransport`] over the session bus. `Ask` answers the path of the
//! answer object; the object's `Updated` signal is subscribed to before its `View` is read, so
//! a change between the two is not lost.

use crate::companion::{CompanionTransport, Follow};
use crate::transport::TransportError;
use companion_wire::{AnswerWire, AskWire};
use docket_core::{SessionOpen, SessionOpened};
use docket_dbus::{BusConnection, CompanionAnswerProxy, CompanionProxy, Details};
use futures_util::{Stream, StreamExt};
use prov::SessionId;
use std::pin::Pin;
use zbus::proxy::CacheProperties;
use zbus::zvariant::OwnedObjectPath;

/// companiond over D-Bus, as the connection's caller.
#[derive(Debug)]
pub struct DbusCompanion {
    connection: BusConnection,
    companion: CompanionProxy<'static>,
}

fn fault_of(error: zbus::Error) -> TransportError {
    match &error {
        zbus::Error::MethodError(name, _, _)
            if matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.NameHasNoOwner"
                    | "org.freedesktop.DBus.Error.Spawn.ServiceNotFound"
                    | "org.freedesktop.DBus.Error.Spawn.ExecFailed"
                    | "org.freedesktop.DBus.Error.Disconnected"
            ) =>
        {
            TransportError::Closed
        }
        zbus::Error::MethodError(name, Some(text), _) => {
            TransportError::Bus(format!("{}: {text}", name.as_str()))
        }
        _ => TransportError::Bus(error.to_string()),
    }
}

fn json<T: serde::Serialize>(value: &T) -> Result<String, TransportError> {
    serde_json::to_string(value).map_err(|e| TransportError::Malformed(e.to_string()))
}

impl DbusCompanion {
    /// Talks to companiond over `connection`; the bus starts it on the first call when it is
    /// installed with an activation file.
    pub async fn over(connection: BusConnection) -> Result<Self, TransportError> {
        let companion = CompanionProxy::new(&connection).await.map_err(fault_of)?;
        Ok(Self {
            connection,
            companion,
        })
    }

    /// Connects to the session bus.
    pub async fn connect() -> Result<Self, TransportError> {
        let connection = BusConnection::session()
            .await
            .map_err(|e| TransportError::Bus(e.to_string()))?;
        Self::over(connection).await
    }
}

/// An answer object being followed over the bus.
pub struct BusAnswer {
    proxy: CompanionAnswerProxy<'static>,
    updates: Pin<Box<dyn Stream<Item = String> + Send>>,
    started: bool,
}

impl std::fmt::Debug for BusAnswer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusAnswer")
    }
}

fn view(text: &str) -> Result<AnswerWire, TransportError> {
    serde_json::from_str(text).map_err(|e| TransportError::Malformed(e.to_string()))
}

impl Follow for BusAnswer {
    async fn next(&mut self) -> Result<Option<AnswerWire>, TransportError> {
        if !self.started {
            self.started = true;
            let text = self.proxy.view().await.map_err(fault_of)?;
            return view(&text).map(Some);
        }
        match self.updates.next().await {
            Some(text) => view(&text).map(Some),
            None => Ok(None),
        }
    }
}

impl CompanionTransport for DbusCompanion {
    type Answer = BusAnswer;

    async fn open(&self, open: &SessionOpen) -> Result<SessionOpened, TransportError> {
        let text = self.companion.open(&json(open)?).await.map_err(fault_of)?;
        serde_json::from_str(&text).map_err(|e| TransportError::Malformed(e.to_string()))
    }

    async fn ask(&self, ask: &AskWire) -> Result<BusAnswer, TransportError> {
        let path: OwnedObjectPath = self
            .companion
            .ask(&json(ask)?, &Details::new())
            .await
            .map_err(fault_of)?;
        let proxy = CompanionAnswerProxy::builder(&self.connection)
            .path(path)
            .map_err(fault_of)?
            .cache_properties(CacheProperties::No)
            .build()
            .await
            .map_err(fault_of)?;
        let updates = proxy
            .receive_updated()
            .await
            .map_err(fault_of)?
            .filter_map(|signal| async move { signal.args().ok().map(|a| a.view().to_string()) });
        Ok(BusAnswer {
            proxy,
            updates: Box::pin(updates),
            started: false,
        })
    }

    async fn close(&self, session: &SessionId) -> Result<(), TransportError> {
        self.companion
            .close(session.as_str())
            .await
            .map_err(fault_of)
    }
}
