//! `org.quire.Intents1.Search`: search over the index and the apps that do not index.

use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Search",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Search {
    /// Starts a search (`SearchScope` JSON); answers the indexed hits (`Vec<Hit>` JSON); late app hits come as `Hits`.
    fn query(&self, text: &str, scope: &str, generation: u64) -> zbus::Result<String>;

    /// Cancels a search.
    fn cancel(&self, generation: u64) -> zbus::Result<()>;

    /// Late hits for a generation (`Vec<Hit>` JSON).
    #[zbus(signal)]
    fn hits(&self, generation: u64, hits: &str) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct SearchSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Search")]
impl SearchSkeleton {
    fn query(&self, text: String, scope: String, generation: u64) -> fdo::Result<String> {
        let _ = (text, scope, generation);
        Err(crate::introspect::frozen())
    }

    fn cancel(&self, generation: u64) -> fdo::Result<()> {
        let _ = (generation,);
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn hits(emitter: &SignalEmitter<'_>, generation: u64, hits: &str) -> zbus::Result<()>;
}
