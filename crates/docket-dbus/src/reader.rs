//! `org.quire.Reader1`: the quarantined reader, served by readerd. Only intentd may call it.

use porter_dbus::Details;
use zbus::fdo;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Reader1",
    default_service = "org.quire.Reader1",
    default_path = "/org/quire/Reader1"
)]
pub trait Reader {
    /// Reads the handles of a session under a schema (`ReaderAsk` JSON; answers `Value` JSON).
    fn extract(&self, session: &str, ask: &str, options: &Details) -> zbus::Result<String>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct ReaderSkeleton;

#[zbus::interface(name = "org.quire.Reader1")]
impl ReaderSkeleton {
    fn extract(&self, session: String, ask: String, options: Details) -> fdo::Result<String> {
        let _ = (session, ask, options);
        Err(crate::introspect::frozen())
    }
}
