//! `org.quire.Intents1.Context`: what the person is looking at.

use zbus::fdo;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Context",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Context {
    /// The context in the window of `app`, the app the person summoned the companion from (an app name), for this session (`ContextView` JSON), untrusted text as handles.
    fn current(&self, session: &str, app: &str) -> zbus::Result<String>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct ContextSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Context")]
impl ContextSkeleton {
    fn current(&self, session: String, app: String) -> fdo::Result<String> {
        let _ = (session, app);
        Err(crate::introspect::frozen())
    }
}
