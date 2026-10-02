//! `org.quire.Intents1.Registry`: the installed manifests.

use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Registry",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Registry {
    /// Every installed app and its manifest (JSON).
    fn manifests(&self) -> zbus::Result<Vec<(String, String)>>;

    /// An app installed, updated or removed its manifest.
    #[zbus(signal)]
    fn manifest_changed(&self, app: &str) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct RegistrySkeleton;

#[zbus::interface(name = "org.quire.Intents1.Registry")]
impl RegistrySkeleton {
    fn manifests(&self) -> fdo::Result<Vec<(String, String)>> {
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn manifest_changed(emitter: &SignalEmitter<'_>, app: &str) -> zbus::Result<()>;
}
