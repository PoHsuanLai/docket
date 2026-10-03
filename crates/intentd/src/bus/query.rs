//! The read-mostly interfaces: the registry, the index, search, context and messages.

use super::{Gateway, json, parsed};
use docket_core::{Generation, IntentsReply, IntentsRequest, SendRefusal, WireRefusal};
use docket_dbus::{Details, IntentsError};
use prov::SessionId;
use zbus::message::Header;
use zbus::object_server::SignalEmitter;

pub(crate) struct RegistryBus(pub(crate) Gateway);

#[zbus::interface(name = "org.quire.Intents1.Registry")]
impl RegistryBus {
    async fn manifests(
        &self,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<Vec<(String, String)>, IntentsError> {
        let all = self
            .0
            .ask(&header, IntentsRequest::Manifests, |r| match r {
                IntentsReply::Manifests(all) => Some(all),
                _ => None,
            })
            .await?;
        all.iter()
            .map(|m| {
                let text = super::render(m)?;
                Ok((m.manifest().app.to_string(), text))
            })
            .collect()
    }

    #[zbus(signal)]
    async fn manifest_changed(emitter: &SignalEmitter<'_>, app: &str) -> zbus::Result<()>;
}

pub(crate) struct IndexBus(pub(crate) Gateway);

#[zbus::interface(name = "org.quire.Intents1.Index")]
impl IndexBus {
    async fn push(
        &self,
        batch: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), IntentsError> {
        self.0
            .done(&header, IntentsRequest::IndexPush(json(&batch)?))
            .await
    }

    async fn reset(
        &self,
        epoch: u64,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), IntentsError> {
        self.0
            .done(&header, IntentsRequest::IndexReset { epoch })
            .await
    }
}

pub(crate) struct SearchBus(pub(crate) Gateway);

#[zbus::interface(name = "org.quire.Intents1.Search")]
impl SearchBus {
    async fn query(
        &self,
        text: String,
        scope: String,
        generation: u64,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> Result<String, IntentsError> {
        let ask = docket_core::SearchAsk {
            text,
            scope: json(&scope)?,
            generation: Generation(generation),
        };
        let caller = self.0.caller(&header).await?;
        let indexed = (self.0.search.indexed)(caller.clone(), &ask)
            .ok_or_else(|| IntentsError::NotAllowed("not for this caller".into()))?;
        if let Some(sender) = header.sender().map(|s| s.to_string()) {
            self.0.late_hits(connection.clone(), sender, caller, ask);
        }
        super::render(&indexed)
    }

    async fn cancel(
        &self,
        generation: u64,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<(), IntentsError> {
        if let Some(sender) = header.sender().map(|s| s.to_string()) {
            self.0.cancel_search(&sender, generation);
        }
        self.0
            .done(
                &header,
                IntentsRequest::SearchCancel(Generation(generation)),
            )
            .await
    }

    #[zbus(signal)]
    async fn hits(emitter: &SignalEmitter<'_>, generation: u64, hits: &str) -> zbus::Result<()>;
}

pub(crate) struct ContextBus(pub(crate) Gateway);

#[zbus::interface(name = "org.quire.Intents1.Context")]
impl ContextBus {
    async fn current(
        &self,
        session: String,
        app: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let request = IntentsRequest::Context {
            session: parsed(&session, SessionId::parse)?,
            app: parsed(&app, porter_core::AppName::parse)?,
        };
        self.0
            .answer(&header, request, |reply| {
                super::or_refused(reply, |r| match r {
                    IntentsReply::Context(view) => Some(*view),
                    _ => None,
                })
            })
            .await
    }
}

pub(crate) struct MessageBus(pub(crate) Gateway);

#[zbus::interface(name = "org.quire.Intents1.Message")]
impl MessageBus {
    async fn send(
        &self,
        session: String,
        draft: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let _ = options;
        let request = IntentsRequest::MessageSend {
            session: parsed(&session, SessionId::parse)?,
            draft: json(&draft)?,
        };
        self.0
            .answer(&header, request, |r| match r {
                IntentsReply::Delivered(d) => Some(Ok(d)),
                IntentsReply::Refused(WireRefusal::Send(why)) => Some(Err::<_, SendRefusal>(why)),
                _ => None,
            })
            .await
    }

    async fn inbox(
        &self,
        ask: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        self.0
            .answer(
                &header,
                IntentsRequest::MessageInbox(json(&ask)?),
                |r| match r {
                    IntentsReply::Inbox(lines) => Some(lines),
                    _ => None,
                },
            )
            .await
    }

    #[zbus(signal)]
    async fn arrived(emitter: &SignalEmitter<'_>, agent: &str) -> zbus::Result<()>;
}
