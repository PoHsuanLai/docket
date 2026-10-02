//! `org.quire.Intents1.Request`: one request in flight, as portals do. `Response` carries 0 (done), 1 (cancelled) or 2 (other) and the reply JSON.

use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Request",
    default_service = "org.quire.Intents1"
)]
pub trait Request {
    /// Withdraws the request; no `Response` follows.
    fn close(&self) -> zbus::Result<()>;

    /// The computer-use lease is suspended: show the confirmation now.
    fn proceed(&self) -> zbus::Result<()>;

    /// How far the call is (`CallProgress` JSON).
    #[zbus(signal)]
    fn progress(&self, progress: &str) -> zbus::Result<()>;

    /// The answer: a code and the reply JSON.
    #[zbus(signal)]
    fn response(&self, code: u32, reply: &str) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct RequestSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Request")]
impl RequestSkeleton {
    fn close(&self) -> fdo::Result<()> {
        Err(crate::introspect::frozen())
    }

    fn proceed(&self) -> fdo::Result<()> {
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn progress(emitter: &SignalEmitter<'_>, progress: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn response(emitter: &SignalEmitter<'_>, code: u32, reply: &str) -> zbus::Result<()>;
}
