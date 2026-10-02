//! `org.quire.Intents1.Context`: what the person is looking at.

use zbus::fdo;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Context",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Context {
    /// The context the session was given at its turn (`ContextView` JSON), untrusted text as handles.
    fn current(&self, session: &str) -> zbus::Result<String>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct ContextSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Context")]
impl ContextSkeleton {
    fn current(&self, session: String) -> fdo::Result<String> {
        let _ = (session,);
        Err(crate::introspect::frozen())
    }
}
