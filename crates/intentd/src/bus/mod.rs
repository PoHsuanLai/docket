//! The bus side of the router: every interface of `org.quire.Intents1` served at
//! `/org/quire/Intents1` for one [`Gateway`], which derives the caller from the connection,
//! hands the request to the router and answers. The members are those of `docket-dbus`'s
//! skeletons (the tests hold the introspection to `dbus/org.quire.Intents1.xml`); the bodies
//! are the JSON of the typed values `docket-client`'s transport reads. No member here carries a
//! doc comment: zbus would copy it into the introspection.

mod control;
mod query;
mod request;
mod run;
mod session;

use crate::peer::{PeerFault, Peers};
use docket_core::{CallerId, Hit, IntentsReply, IntentsRequest, SearchAsk};
use docket_dbus::{INTENTS_PATH, IntentsError};
use docket_router::Watch;
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::collections::BTreeMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};
use zbus::message::Header;

pub(crate) use control::ControlBus;
pub(crate) use query::{ContextBus, IndexBus, MessageBus, RegistryBus, SearchBus};
pub(crate) use run::{GateBus, RunBus};
pub(crate) use session::SessionBus;

/// The router, as the bus sees it: one request from one caller, one reply.
pub(crate) type Handler = Arc<
    dyn Fn(CallerId, IntentsRequest, Watch) -> Pin<Box<dyn Future<Output = IntentsReply> + Send>>
        + Send
        + Sync,
>;

/// What the shadow index answers at once: `None` when the caller's role may not search.
type IndexedSearch = Arc<dyn Fn(CallerId, &SearchAsk) -> Option<Vec<Hit>> + Send + Sync>;

/// What the apps that do not index answer, possibly late.
type LateSearch = Arc<
    dyn Fn(CallerId, SearchAsk) -> Pin<Box<dyn Future<Output = Vec<Hit>> + Send>> + Send + Sync,
>;

/// A search, split in two: what the shadow index answers at once (`None` when the caller's role
/// may not search), and what the apps that do not index must be asked, which may come late.
#[derive(Clone)]
pub(crate) struct SearchPort {
    pub(crate) indexed: IndexedSearch,
    pub(crate) late: LateSearch,
}

/// The generation of each asker's newest search that has not been cancelled: late hits of any
/// other generation are stale and are not sent.
pub(crate) type LiveSearches = Arc<Mutex<BTreeMap<String, u64>>>;

/// What every interface shares.
#[derive(Clone)]
pub(crate) struct Gateway {
    handler: Handler,
    search: SearchPort,
    live: LiveSearches,
    peers: Peers,
    requests: Arc<AtomicU64>,
}

impl std::fmt::Debug for Gateway {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Gateway")
            .field("peers", &self.peers)
            .finish()
    }
}

/// A body that is not the JSON of its type is malformed.
pub(crate) fn json<T: DeserializeOwned>(text: &str) -> Result<T, IntentsError> {
    serde_json::from_str(text).map_err(|e| IntentsError::Malformed(e.to_string()))
}

/// The JSON of an answer.
pub(crate) fn render<T: Serialize>(value: &T) -> Result<String, IntentsError> {
    serde_json::to_string(value).map_err(|e| IntentsError::Malformed(e.to_string()))
}

/// A reply read by `pick`, or the refusal of a call that the router answered in its place: the
/// body of a member whose request can be refused as a call (`DryRun`, `Preview`, `Suggest`,
/// `Context`) is `Result<T, CallRefusal>`.
pub(crate) fn or_refused<T>(
    reply: IntentsReply,
    pick: impl FnOnce(IntentsReply) -> Option<T>,
) -> Option<Result<T, docket_core::CallRefusal>> {
    match reply {
        IntentsReply::Refused(docket_core::WireRefusal::Call(why)) => Some(Err(why)),
        other => pick(other).map(Ok),
    }
}

/// A text id of the wire.
pub(crate) fn parsed<T, E>(
    text: &str,
    parse: impl FnOnce(&str) -> Result<T, E>,
) -> Result<T, IntentsError> {
    parse(text).map_err(|_| IntentsError::Malformed(format!("not an id: {text}")))
}

impl Gateway {
    /// A gateway over `handler` that names callers with `peers`.
    pub(crate) fn new(handler: Handler, search: SearchPort, peers: Peers) -> Self {
        Self {
            handler,
            search,
            live: LiveSearches::default(),
            peers,
            requests: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Starts the part of a search that has to ask apps, for `sender` alone: its hits come as
    /// `Hits` for `ask.generation` unless that search was cancelled or a newer one replaced it.
    pub(crate) fn late_hits(
        &self,
        connection: zbus::Connection,
        sender: String,
        caller: CallerId,
        ask: SearchAsk,
    ) {
        let generation = ask.generation.0;
        self.live
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(sender.clone(), generation);
        let (late, live) = (self.search.late.clone(), self.live.clone());
        tokio::spawn(async move {
            let hits = late(caller, ask).await;
            let current = live
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .get(&sender)
                .copied();
            if hits.is_empty() || current != Some(generation) {
                return;
            }
            let Ok(text) = serde_json::to_string(&hits) else {
                return;
            };
            let Ok(to) = zbus::names::BusName::try_from(sender.as_str()) else {
                return;
            };
            let _ = connection
                .emit_signal(
                    Some(to),
                    INTENTS_PATH,
                    "org.quire.Intents1.Search",
                    "Hits",
                    &(generation, text),
                )
                .await;
        });
    }

    /// The asker no longer wants the late hits of `generation`.
    pub(crate) fn cancel_search(&self, sender: &str, generation: u64) {
        let mut live = self.live.lock().unwrap_or_else(|p| p.into_inner());
        if live.get(sender) == Some(&generation) {
            live.remove(sender);
        }
    }

    /// The caller behind the message: derived from its connection, never from a body.
    pub(crate) async fn caller(&self, header: &Header<'_>) -> Result<CallerId, IntentsError> {
        let sender = header
            .sender()
            .ok_or_else(|| IntentsError::NotAllowed("no sender".into()))?;
        self.peers
            .caller(sender.as_str())
            .await
            .map_err(|why| match why {
                PeerFault::Bus(text) => IntentsError::ZBus(zbus::Error::Failure(text)),
                other => IntentsError::NotAllowed(other.to_string()),
            })
    }

    /// Hands the request to the router as the caller of `header`.
    pub(crate) async fn reply(
        &self,
        header: &Header<'_>,
        request: IntentsRequest,
    ) -> Result<IntentsReply, IntentsError> {
        let caller = self.caller(header).await?;
        Ok((self.handler)(caller, request, Watch::none()).await)
    }

    /// The reply read by `pick`: a request refused before it was a call is the bus error, and
    /// a reply of another kind is malformed.
    pub(crate) async fn ask<R>(
        &self,
        header: &Header<'_>,
        request: IntentsRequest,
        pick: impl FnOnce(IntentsReply) -> Option<R>,
    ) -> Result<R, IntentsError> {
        match self.reply(header, request).await? {
            IntentsReply::Refused(why) if IntentsError::from_refusal(&why).is_some() => {
                Err(IntentsError::from_refusal(&why)
                    .unwrap_or(IntentsError::Malformed(String::new())))
            }
            reply => pick(reply).ok_or_else(|| IntentsError::Malformed("unexpected reply".into())),
        }
    }

    /// `ask`, answered as the JSON of what `pick` read.
    pub(crate) async fn answer<R: Serialize>(
        &self,
        header: &Header<'_>,
        request: IntentsRequest,
        pick: impl FnOnce(IntentsReply) -> Option<R>,
    ) -> Result<String, IntentsError> {
        render(&self.ask(header, request, pick).await?)
    }

    /// `ask` for a member that answers nothing.
    pub(crate) async fn done(
        &self,
        header: &Header<'_>,
        request: IntentsRequest,
    ) -> Result<(), IntentsError> {
        self.ask(header, request, |r| {
            matches!(r, IntentsReply::Done).then_some(())
        })
        .await
    }
}

/// Exports every interface of `Intents1` on `connection` at its root object.
pub(crate) async fn export(connection: &zbus::Connection, gateway: &Gateway) -> zbus::Result<()> {
    let server = connection.object_server();
    server
        .at(INTENTS_PATH, RegistryBus(gateway.clone()))
        .await?;
    server.at(INTENTS_PATH, IndexBus(gateway.clone())).await?;
    server.at(INTENTS_PATH, SearchBus(gateway.clone())).await?;
    server.at(INTENTS_PATH, RunBus(gateway.clone())).await?;
    server.at(INTENTS_PATH, ContextBus(gateway.clone())).await?;
    server.at(INTENTS_PATH, SessionBus(gateway.clone())).await?;
    server.at(INTENTS_PATH, MessageBus(gateway.clone())).await?;
    server.at(INTENTS_PATH, GateBus(gateway.clone())).await?;
    server.at(INTENTS_PATH, ControlBus(gateway.clone())).await?;
    Ok(())
}
