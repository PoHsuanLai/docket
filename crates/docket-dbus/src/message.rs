//! `org.quire.Intents1.Message`: messages between agents, and between the person and an agent: one model, `prov::Message`.

use porter_dbus::Details;
use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Message",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Message {
    /// Stamps and delivers a draft (`MessageDraft` JSON; answers `Delivery` JSON).
    fn send(&self, session: &str, draft: &str, options: &Details) -> zbus::Result<String>;

    /// The messages that wait for an agent (`InboxAsk` JSON; answers `Vec<InboundLine>` JSON).
    fn inbox(&self, ask: &str) -> zbus::Result<String>;

    /// A message arrived for this agent (`AgentRef` JSON). Content-free: the receiver reads its inbox.
    #[zbus(signal)]
    fn arrived(&self, agent: &str) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct MessageSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Message")]
impl MessageSkeleton {
    fn send(&self, session: String, draft: String, options: Details) -> fdo::Result<String> {
        let _ = (session, draft, options);
        Err(crate::introspect::frozen())
    }

    fn inbox(&self, ask: String) -> fdo::Result<String> {
        let _ = (ask,);
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn arrived(emitter: &SignalEmitter<'_>, agent: &str) -> zbus::Result<()>;
}
